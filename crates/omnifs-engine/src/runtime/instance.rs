//! `Instance` — the Wasmtime mechanics boundary.
//!
//! Owns the wasm store, the generated bindings, and the serialized
//! provider config. A dedicated driver thread keeps Wasmtime's concurrent
//! store event loop alive so independent host tasks can start provider
//! calls while earlier calls are suspended on async host imports.
//! `Runtime` composes this with orchestration concerns
//! (executors, caches, activity, invalidation, coalesce).

use std::future::Future;
use std::path::{Component as PathComponent, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::authority::RuntimeAuthority;
use crate::callouts::{CalloutHost, ParkSignal};
use futures::StreamExt;
use tracing::Instrument;
use wasmtime::component::{Linker, ResourceTable};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtxBuilder};

use crate::Provider;
use crate::runtime::wasm::ComponentEngine;
use crate::wasi::HostState;
use crate::{BuildError, EngineError};
use omnifs_wit::host::types as wit_types;

const COMMAND_QUEUE_CAPACITY: usize = 64;
const MAX_IN_FLIGHT_OPERATIONS: usize = 32;
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct Instance {
    tx: tokio::sync::mpsc::Sender<Command>,
    admission: Arc<tokio::sync::Semaphore>,
    shutting_down: Arc<std::sync::atomic::AtomicBool>,
    config_bytes: Vec<u8>,
}

struct OperationEnvelope<C, R> {
    command: C,
    span: tracing::Span,
    reply: tokio::sync::oneshot::Sender<R>,
    cancel: tokio::sync::oneshot::Receiver<()>,
    permit: tokio::sync::OwnedSemaphorePermit,
}

struct LookupChildCommand {
    id: u64,
    parent_path: String,
    name: String,
}

struct ListChildrenCommand {
    id: u64,
    path: String,
    cursor: Option<wit_types::Cursor>,
}

struct ReadFileCommand {
    id: u64,
    path: String,
    content_type: String,
    cached_canonical: Option<wit_types::CanonicalInput>,
}

struct OpenFileCommand {
    id: u64,
    path: String,
}

struct ReadChunkCommand {
    id: u64,
    handle: u64,
    offset: u64,
    length: u32,
}

struct EventCommand {
    id: u64,
    event: wit_types::ProviderEvent,
}

enum Command {
    SetCallouts {
        callouts: CalloutHost,
        reply: std::sync::mpsc::Sender<std::result::Result<(), EngineError>>,
    },
    Initialize {
        config_bytes: Vec<u8>,
        reply: std::sync::mpsc::Sender<InitializeTransport>,
    },
    LookupChild(OperationEnvelope<LookupChildCommand, LookupTransport>),
    ListChildren(OperationEnvelope<ListChildrenCommand, ListTransport>),
    ReadFile(OperationEnvelope<ReadFileCommand, ReadTransport>),
    OpenFile(OperationEnvelope<OpenFileCommand, OpenTransport>),
    ReadChunk(OperationEnvelope<ReadChunkCommand, ChunkTransport>),
    OnEvent(OperationEnvelope<EventCommand, EventTransport>),
    Shutdown {
        reply: std::sync::mpsc::Sender<std::result::Result<(), EngineError>>,
    },
    CloseFile {
        handle: u64,
        reply: std::sync::mpsc::Sender<std::result::Result<(), EngineError>>,
    },
}

type InitializeTransport = std::result::Result<
    (
        std::result::Result<(), wit_types::ProviderError>,
        wit_types::Effects,
    ),
    EngineError,
>;
type LookupTransport = std::result::Result<
    (
        std::result::Result<wit_types::LookupChildResult, wit_types::ProviderError>,
        wit_types::Effects,
    ),
    EngineError,
>;
type ListTransport = std::result::Result<
    (
        std::result::Result<wit_types::ListChildrenResult, wit_types::ProviderError>,
        wit_types::Effects,
    ),
    EngineError,
>;
type ReadTransport = std::result::Result<
    (
        std::result::Result<wit_types::ReadFileOutcome, wit_types::ProviderError>,
        wit_types::Effects,
    ),
    EngineError,
>;
type OpenTransport = std::result::Result<
    (
        std::result::Result<wit_types::OpenFileResult, wit_types::ProviderError>,
        wit_types::Effects,
    ),
    EngineError,
>;
type ChunkTransport = std::result::Result<
    (
        std::result::Result<wit_types::ReadChunkResult, wit_types::ProviderError>,
        wit_types::Effects,
    ),
    EngineError,
>;
type EventTransport = std::result::Result<
    (
        std::result::Result<(), wit_types::ProviderError>,
        wit_types::Effects,
    ),
    EngineError,
>;

impl Instance {
    pub(crate) fn new(
        engine: &ComponentEngine,
        component_bytes: Arc<[u8]>,
        config_bytes: Vec<u8>,
        authority: Arc<RuntimeAuthority>,
        park_signal: Option<ParkSignal>,
    ) -> std::result::Result<Self, BuildError> {
        let (tx, rx) = tokio::sync::mpsc::channel(COMMAND_QUEUE_CAPACITY);
        let admission = Arc::new(tokio::sync::Semaphore::new(MAX_IN_FLIGHT_OPERATIONS));
        let shutting_down = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let engine = engine.clone();

        std::thread::Builder::new()
            .name("omnifs-provider-instance".to_string())
            .spawn(move || {
                let mut builder = tokio::runtime::Builder::new_current_thread();
                builder.enable_all();
                // Test capture only: signal the harness each time this
                // single-threaded executor goes idle, so it can close a
                // captured callout burst on the executor's real quiescence
                // boundary rather than a timing heuristic. `None` in
                // production, where nothing observes callout bursts.
                if let Some(park_signal) = park_signal {
                    builder.on_thread_park(move || park_signal.notify());
                }
                let runtime = match builder.build() {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = ready_tx.send(Err(BuildError::ProviderProtocol(format!(
                            "provider driver runtime: {error}"
                        ))));
                        return;
                    },
                };
                runtime.block_on(async move {
                    match build_driver_state(&engine, &component_bytes, &authority).await {
                        Ok((store, bindings)) => {
                            let _ = ready_tx.send(Ok(()));
                            if let Err(error) = drive_instance(store, bindings, rx).await {
                                tracing::error!(error = %error, "provider instance driver exited");
                            }
                        },
                        Err(error) => {
                            let _ = ready_tx.send(Err(error));
                        },
                    }
                });
            })
            .map_err(|error| {
                BuildError::ProviderProtocol(format!("spawn provider driver: {error}"))
            })?;

        ready_rx.recv().map_err(|error| {
            BuildError::ProviderProtocol(format!("provider driver did not start: {error}"))
        })??;

        Ok(Self {
            tx,
            admission,
            shutting_down,
            config_bytes,
        })
    }

    async fn submit<C, T>(
        &self,
        command: C,
        build: impl FnOnce(OperationEnvelope<C, T>) -> Command,
    ) -> std::result::Result<T, EngineError>
    where
        C: Send + 'static,
        T: Send + 'static,
    {
        if self
            .shutting_down
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(EngineError::ProviderAdmission(
                "provider runtime is shutting down".to_owned(),
            ));
        }
        let permit = self.admission.clone().try_acquire_owned().map_err(|_| {
            EngineError::ProviderAdmission("provider operation in-flight limit reached".to_owned())
        })?;
        let (reply, receive) = tokio::sync::oneshot::channel();
        let (cancel, cancellation) = tokio::sync::oneshot::channel();
        self.tx
            .try_send(build(OperationEnvelope {
                command,
                span: tracing::Span::current(),
                reply,
                cancel: cancellation,
                permit,
            }))
            .map_err(|error| match error {
                tokio::sync::mpsc::error::TrySendError::Full(_) => EngineError::ProviderAdmission(
                    "provider operation queue capacity reached".to_owned(),
                ),
                tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                    EngineError::ProviderProtocol("provider instance driver stopped".to_owned())
                },
            })?;
        let result = receive.await.map_err(|_| {
            EngineError::ProviderProtocol("provider operation reply dropped".to_owned())
        });
        drop(cancel);
        result
    }

    pub(crate) async fn lookup_child(
        &self,
        id: u64,
        parent_path: String,
        name: String,
    ) -> LookupTransport {
        self.submit(
            LookupChildCommand {
                id,
                parent_path,
                name,
            },
            Command::LookupChild,
        )
        .await?
    }

    pub(crate) async fn list_children(
        &self,
        id: u64,
        path: String,
        cursor: Option<wit_types::Cursor>,
    ) -> ListTransport {
        self.submit(
            ListChildrenCommand { id, path, cursor },
            Command::ListChildren,
        )
        .await?
    }

    pub(crate) async fn read_file(
        &self,
        id: u64,
        path: String,
        content_type: String,
        cached_canonical: Option<wit_types::CanonicalInput>,
    ) -> ReadTransport {
        self.submit(
            ReadFileCommand {
                id,
                path,
                content_type,
                cached_canonical,
            },
            Command::ReadFile,
        )
        .await?
    }

    pub(crate) async fn open_file(&self, id: u64, path: String) -> OpenTransport {
        self.submit(OpenFileCommand { id, path }, Command::OpenFile)
            .await?
    }

    pub(crate) async fn read_chunk(
        &self,
        id: u64,
        handle: u64,
        offset: u64,
        length: u32,
    ) -> ChunkTransport {
        self.submit(
            ReadChunkCommand {
                id,
                handle,
                offset,
                length,
            },
            Command::ReadChunk,
        )
        .await?
    }

    pub(crate) async fn on_event(
        &self,
        id: u64,
        event: wit_types::ProviderEvent,
    ) -> EventTransport {
        self.submit(EventCommand { id, event }, Command::OnEvent)
            .await?
    }

    pub fn initialize(&self) -> InitializeTransport {
        self.call_sync(|reply| Command::Initialize {
            config_bytes: self.config_bytes.clone(),
            reply,
        })
    }

    pub(crate) fn set_callouts(
        &self,
        callouts: CalloutHost,
    ) -> std::result::Result<(), EngineError> {
        self.call_sync(|reply| Command::SetCallouts { callouts, reply })
    }

    pub fn shutdown(&self) -> std::result::Result<(), EngineError> {
        if self
            .shutting_down
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return Ok(());
        }
        self.call_sync_unchecked(|reply| Command::Shutdown { reply })
    }

    pub fn close_file(&self, handle: u64) -> std::result::Result<(), EngineError> {
        self.call_sync(|reply| Command::CloseFile { handle, reply })
    }

    fn call_sync<T>(
        &self,
        build: impl FnOnce(std::sync::mpsc::Sender<std::result::Result<T, EngineError>>) -> Command,
    ) -> std::result::Result<T, EngineError> {
        if self
            .shutting_down
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(EngineError::ProviderAdmission(
                "provider runtime is shutting down".to_owned(),
            ));
        }
        self.call_sync_unchecked(build)
    }

    fn call_sync_unchecked<T>(
        &self,
        build: impl FnOnce(std::sync::mpsc::Sender<std::result::Result<T, EngineError>>) -> Command,
    ) -> std::result::Result<T, EngineError> {
        let (reply, recv) = std::sync::mpsc::channel();
        self.tx
            .try_send(build(reply))
            .map_err(|error| match error {
                tokio::sync::mpsc::error::TrySendError::Full(_) => EngineError::ProviderAdmission(
                    "provider control queue capacity reached".to_owned(),
                ),
                tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                    EngineError::ProviderProtocol("provider instance driver stopped".to_owned())
                },
            })?;
        recv.recv().map_err(|_| {
            EngineError::ProviderProtocol("provider instance reply dropped".to_string())
        })?
    }
}

async fn build_driver_state(
    engine: &ComponentEngine,
    component_bytes: &[u8],
    authority: &RuntimeAuthority,
) -> std::result::Result<(wasmtime::Store<HostState>, Provider), BuildError> {
    let mut linker = Linker::<HostState>::new(engine.inner());
    wasmtime_wasi::p2::add_to_linker_async::<HostState>(&mut linker)?;
    Provider::add_to_linker::<HostState, HostState>(&mut linker, |state| state)?;

    let component = engine.load(component_bytes)?;
    let wasi = build_wasi_ctx(authority)?;
    let mut store = wasmtime::Store::new(
        engine.inner(),
        HostState {
            wasi,
            table: ResourceTable::new(),
            callouts: None,
        },
    );

    let bindings = Provider::instantiate_async(&mut store, &component, &linker).await?;
    Ok((store, bindings))
}

// Keep the Wasmtime command driver cohesive: its ordered command dispatch and
// in-flight future polling are one runtime boundary.
#[allow(clippy::too_many_lines)]
async fn drive_instance(
    mut store: wasmtime::Store<HostState>,
    bindings: Provider,
    mut rx: tokio::sync::mpsc::Receiver<Command>,
) -> wasmtime::Result<()> {
    let bindings = Arc::new(bindings);
    store
        .run_concurrent(async |accessor| -> wasmtime::Result<()> {
            let mut calls: futures::stream::FuturesUnordered<Pin<Box<dyn Future<Output = ()>>>> =
                futures::stream::FuturesUnordered::new();
            loop {
                tokio::select! {
                    Some(command) = rx.recv() => {
                        match command {
                            Command::SetCallouts { callouts, reply } => {
                                accessor.with(|mut access| {
                                    access.get().callouts = Some(callouts);
                                });
                                let _ = reply.send(Ok(()));
                            },
                            Command::Initialize { config_bytes, reply } => {
                                let lifecycle = bindings.omnifs_provider_lifecycle();
                                let result = match accessor.with(|access| {
                                    lifecycle
                                        .func_initialize()
                                        .func()
                                        .typed::<
                                            (Vec<u8>,),
                                            ((std::result::Result<(), wit_types::ProviderError>, wit_types::Effects),),
                                        >(&access)
                                }) {
                                    Ok(initialize) => initialize
                                        .call_concurrent(accessor, (config_bytes,))
                                        .await
                                        .map(|(ret,)| ret),
                                    Err(error) => Err(error),
                                }
                                .map_err(Into::into);
                                let _ = reply.send(result);
                            },
                            Command::LookupChild(OperationEnvelope {
                                command: LookupChildCommand { id, parent_path, name },
                                span,
                                reply,
                                cancel,
                                permit,
                            }) => {
                                let namespace = Arc::clone(&bindings);
                                calls.push(Box::pin(run_operation(cancel, reply, permit, async move {
                                    namespace
                                        .omnifs_provider_namespace()
                                        .call_lookup_child(accessor, id, parent_path, name)
                                        .await
                                        .map_err(Into::into)
                                }.instrument(span))));
                            },
                            Command::ListChildren(OperationEnvelope {
                                command: ListChildrenCommand { id, path, cursor },
                                span,
                                reply,
                                cancel,
                                permit,
                            }) => {
                                let namespace = Arc::clone(&bindings);
                                calls.push(Box::pin(run_operation(cancel, reply, permit, async move {
                                    namespace
                                        .omnifs_provider_namespace()
                                        .call_list_children(accessor, id, path, cursor)
                                        .await
                                        .map_err(Into::into)
                                }.instrument(span))));
                            },
                            Command::ReadFile(OperationEnvelope {
                                command: ReadFileCommand {
                                    id,
                                    path,
                                    content_type,
                                    cached_canonical,
                                },
                                span,
                                reply,
                                cancel,
                                permit,
                            }) => {
                                let namespace = Arc::clone(&bindings);
                                calls.push(Box::pin(run_operation(cancel, reply, permit, async move {
                                    namespace
                                        .omnifs_provider_namespace()
                                        .call_read_file(accessor, id, path, content_type, cached_canonical)
                                        .await
                                        .map_err(Into::into)
                                }.instrument(span))));
                            },
                            Command::OpenFile(OperationEnvelope {
                                command: OpenFileCommand { id, path },
                                span,
                                reply,
                                cancel,
                                permit,
                            }) => {
                                let namespace = Arc::clone(&bindings);
                                calls.push(Box::pin(run_operation(cancel, reply, permit, async move {
                                    namespace
                                        .omnifs_provider_namespace()
                                        .call_open_file(accessor, id, path)
                                        .await
                                        .map_err(Into::into)
                                }.instrument(span))));
                            },
                            Command::ReadChunk(OperationEnvelope {
                                command: ReadChunkCommand {
                                    id,
                                    handle,
                                    offset,
                                    length,
                                },
                                span,
                                reply,
                                cancel,
                                permit,
                            }) => {
                                let namespace = Arc::clone(&bindings);
                                calls.push(Box::pin(run_operation(cancel, reply, permit, async move {
                                    namespace
                                        .omnifs_provider_namespace()
                                        .call_read_chunk(accessor, id, handle, offset, length)
                                        .await
                                        .map_err(Into::into)
                                }.instrument(span))));
                            },
                            Command::OnEvent(OperationEnvelope {
                                command: EventCommand { id, event },
                                span,
                                reply,
                                cancel,
                                permit,
                            }) => {
                                let notify = Arc::clone(&bindings);
                                calls.push(Box::pin(run_operation(cancel, reply, permit, async move {
                                    notify
                                        .omnifs_provider_notify()
                                        .call_on_event(accessor, id, event)
                                        .await
                                        .map_err(Into::into)
                                }.instrument(span))));
                            },
                            Command::Shutdown { reply } => {
                                let drained = tokio::time::timeout(
                                    SHUTDOWN_DRAIN_TIMEOUT,
                                    async {
                                        while calls.next().await.is_some() {}
                                    },
                                )
                                .await
                                .is_ok();
                                let result = if drained {
                                    let shutdown =
                                        bindings.omnifs_provider_lifecycle().func_shutdown();
                                    shutdown
                                        .call_concurrent(accessor, ())
                                        .await
                                        .map_err(Into::into)
                                } else {
                                    Err(EngineError::ProviderProtocol(
                                        "provider operation drain timed out during shutdown"
                                            .to_owned(),
                                    ))
                                };
                                let _ = reply.send(result);
                                break;
                            },
                            Command::CloseFile { handle, reply } => {
                                let close_file =
                                    bindings.omnifs_provider_namespace().func_close_file();
                                let result = close_file
                                    .call_concurrent(accessor, (handle,))
                                    .await
                                    .map_err(Into::into);
                                let _ = reply.send(result);
                            },
                        }
                    },
                    Some(()) = calls.next(), if !calls.is_empty() => {},
                    else => break,
                }
            }
            Ok(())
        })
        .await?
}

async fn run_operation<T, F>(
    cancel: tokio::sync::oneshot::Receiver<()>,
    reply: tokio::sync::oneshot::Sender<T>,
    _permit: tokio::sync::OwnedSemaphorePermit,
    operation: F,
) where
    F: Future<Output = T>,
{
    tokio::select! {
        result = operation => {
            let _ = reply.send(result);
        }
        _ = cancel => {},
    }
}

fn build_wasi_ctx(
    authority: &RuntimeAuthority,
) -> std::result::Result<wasmtime_wasi::WasiCtx, BuildError> {
    let mut builder = WasiCtxBuilder::new();
    for entry in authority.preopens() {
        let host = validate_preopen_path(&entry.host)?;
        // Guest paths must also be absolute. They share the same
        // no-parent-escape rule because Wasmtime's preopen API treats
        // them as opaque mount tokens; relative or `..`-laden values
        // would silently confuse later guest-side path resolution.
        let _ = validate_preopen_path(&entry.guest)?;
        let (dir_perms, file_perms) = match entry.mode {
            omnifs_provider::PreopenMode::Ro => (DirPerms::READ, FilePerms::READ),
            omnifs_provider::PreopenMode::Rw => (
                DirPerms::READ | DirPerms::MUTATE,
                FilePerms::READ | FilePerms::WRITE,
            ),
        };
        builder
            .preopened_dir(&host, &entry.guest, dir_perms, file_perms)
            .map_err(|_| BuildError::InvalidConfig("preopen setup failed".to_owned()))?;
    }
    Ok(builder.build())
}

fn validate_preopen_path(raw: &str) -> std::result::Result<PathBuf, BuildError> {
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err(BuildError::InvalidConfig(
            "preopen path validation failed: path must be absolute".to_owned(),
        ));
    }
    if path
        .components()
        .any(|c| matches!(c, PathComponent::ParentDir))
    {
        return Err(BuildError::InvalidConfig(
            "preopen path validation failed: parent segments are not allowed".to_owned(),
        ));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelled_operation_drops_reply_and_releases_permit() {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let operation = run_operation(cancel_rx, reply_tx, permit, async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            42_u8
        });
        cancel_tx.send(()).unwrap();
        operation.await;
        assert!(reply_rx.await.is_err());
        assert_eq!(semaphore.available_permits(), 1);
    }

    #[tokio::test]
    async fn completed_operation_sends_typed_reply() {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let (_cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        run_operation(cancel_rx, reply_tx, permit, async { 42_u8 }).await;
        assert_eq!(reply_rx.await.unwrap(), 42);
        assert_eq!(semaphore.available_permits(), 1);
    }

    #[test]
    fn validate_preopen_path_rejects_relative() {
        let err = validate_preopen_path("data/db").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("path must be absolute"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn validate_preopen_path_rejects_parent_dir() {
        let err = validate_preopen_path("/data/../etc/passwd").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("parent segments are not allowed"),
            "unexpected error: {msg}"
        );
    }
}
