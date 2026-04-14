//! Effect runtime: WASM provider execution and effect handling.
//!
//! Manages the Wasmtime store, routes provider effect requests to host
//! implementations (HTTP, Git, KV), and handles async continuations.

pub mod capability;
pub mod cloner;
pub mod correlation;
pub mod executor;
pub mod git;

use crate::Provider;
use crate::auth::AuthManager;
use crate::cache::l2::BrowseCacheL2;
use crate::cache::{CacheRecord, RecordKind};
use crate::config::InstanceConfig;
use crate::config::schema::{self, SchemaField};
use crate::omnifs::provider::types as wit_types;
use crate::omnifs::provider::types::DirListing;
use crate::runtime::capability::{CapabilityChecker, CapabilityGrants};
use crate::runtime::cloner::GitCloner;
use crate::runtime::correlation::CorrelationTracker;
use crate::runtime::executor::{
    EffectResponse, ErrorKind, GitEntryKind, HttpExecutor, MemoryKvExecutor,
};
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;

/// Runtime for executing WASM provider components.
///
/// Manages the Wasmtime store, routes effect requests, and handles
/// async continuations with correlation tracking.
pub struct EffectRuntime {
    store: Mutex<wasmtime::Store<HostState>>,
    bindings: Provider,
    correlations: CorrelationTracker,
    http: HttpExecutor,
    kv: MemoryKvExecutor,
    git: git::GitExecutor,
    l2: Option<BrowseCacheL2>,
    invalidated_prefixes: Mutex<Vec<String>>,
}

struct HostState {
    wasi: wasmtime_wasi::WasiCtx,
    table: wasmtime::component::ResourceTable,
}

impl wasmtime_wasi::WasiView for HostState {
    fn ctx(&mut self) -> wasmtime_wasi::WasiCtxView<'_> {
        wasmtime_wasi::WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl crate::omnifs::provider::types::Host for HostState {}
impl crate::omnifs::provider::log::Host for HostState {
    fn log(&mut self, entry: wit_types::LogEntry) {
        match entry.level {
            wit_types::LogLevel::Trace => tracing::trace!("{}", entry.message),
            wit_types::LogLevel::Debug => tracing::debug!("{}", entry.message),
            wit_types::LogLevel::Info => tracing::info!("{}", entry.message),
            wit_types::LogLevel::Warn => tracing::warn!("{}", entry.message),
            wit_types::LogLevel::Error => tracing::error!("{}", entry.message),
        }
    }
}

// Helper type for wasmtime's add_to_linker generic D parameter.
struct HostData;
impl wasmtime::component::HasData for HostData {
    type Data<'a> = &'a mut HostState;
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("wasmtime error: {0}")]
    Wasmtime(#[from] wasmtime::Error),
    #[error("provider returned error: {0}")]
    ProviderError(String),
    #[error("unexpected response type")]
    UnexpectedResponse,
}

impl EffectRuntime {
    pub fn new(
        engine: &wasmtime::Engine,
        wasm_path: &Path,
        config: &InstanceConfig,
        cloner: Arc<GitCloner>,
        cache_dir: &Path,
        mount_name: &str,
    ) -> Result<Self, RuntimeError> {
        let mut linker = wasmtime::component::Linker::<HostState>::new(engine);

        wasmtime_wasi::p2::add_to_linker_sync::<HostState>(&mut linker)?;
        Provider::add_to_linker::<HostState, HostData>(&mut linker, |state: &mut HostState| state)?;

        let component = wasmtime::component::Component::from_file(engine, wasm_path)?;
        let wasi = wasmtime_wasi::WasiCtxBuilder::new().build();
        let mut store = wasmtime::Store::new(
            engine,
            HostState {
                wasi,
                table: wasmtime::component::ResourceTable::new(),
            },
        );

        let bindings = Provider::instantiate(&mut store, &component, &linker)?;

        // Query the provider's declared capabilities and incorporate needs_git.
        let provider_caps = bindings
            .omnifs_provider_lifecycle()
            .call_capabilities(&mut store)?;

        let grants = build_grants(config, provider_caps.needs_git);
        let capability = Arc::new(CapabilityChecker::new(grants));

        // Validate instance config against the provider's declared schema.
        let wit_schema = bindings
            .omnifs_provider_lifecycle()
            .call_get_config_schema(&mut store)?;
        validate_instance_config(&wit_schema, config);
        let auth = if config.auth.is_empty() {
            Arc::new(AuthManager::none())
        } else {
            Arc::new(
                AuthManager::from_configs(&config.auth)
                    .map_err(|e| RuntimeError::ProviderError(format!("auth config error: {e}")))?,
            )
        };

        let git = git::GitExecutor::new(cloner, capability.clone());

        let l2 = {
            let db_path = cache_dir
                .join("providers")
                .join(mount_name)
                .join("browse.redb");
            match BrowseCacheL2::open(&db_path) {
                Ok(cache) => Some(cache),
                Err(e) => {
                    tracing::warn!(mount = mount_name, error = %e, "failed to open L2 browse cache");
                    None
                }
            }
        };

        Ok(Self {
            store: Mutex::new(store),
            bindings,
            correlations: CorrelationTracker::new(),
            http: HttpExecutor::new(auth, capability),
            kv: MemoryKvExecutor::new(),
            git,
            l2,
            invalidated_prefixes: Mutex::new(Vec::new()),
        })
    }

    pub fn initialize(&self, config_bytes: &[u8]) -> Result<wit_types::ActionResult, RuntimeError> {
        let response = {
            let mut store = self.store.lock();
            self.bindings
                .omnifs_provider_lifecycle()
                .call_initialize(&mut *store, config_bytes)?
        };
        Self::resolve_response_sync(response)
    }

    pub fn shutdown(&self) -> Result<(), RuntimeError> {
        let mut store = self.store.lock();
        self.bindings
            .omnifs_provider_lifecycle()
            .call_shutdown(&mut *store)?;
        Ok(())
    }

    pub fn config_schema(&self) -> Result<wit_types::ConfigSchema, RuntimeError> {
        let mut store = self.store.lock();
        Ok(self
            .bindings
            .omnifs_provider_lifecycle()
            .call_get_config_schema(&mut *store)?)
    }

    pub fn capabilities(&self) -> Result<wit_types::RequestedCapabilities, RuntimeError> {
        let mut store = self.store.lock();
        Ok(self
            .bindings
            .omnifs_provider_lifecycle()
            .call_capabilities(&mut *store)?)
    }

    pub async fn call_lookup_child(
        &self,
        parent_path: &str,
        name: &str,
    ) -> Result<wit_types::ActionResult, RuntimeError> {
        let id = self.correlations.allocate();
        self.correlations.mark_pending(id, "lookup_child".into());

        let response = {
            let mut store = self.store.lock();
            self.bindings.omnifs_provider_browse().call_lookup_child(
                &mut *store,
                id,
                parent_path,
                name,
            )?
        };

        self.drive_effects(id, response).await
    }

    pub async fn call_list_children(
        &self,
        path: &str,
    ) -> Result<wit_types::ActionResult, RuntimeError> {
        let id = self.correlations.allocate();
        self.correlations.mark_pending(id, "list_children".into());

        let response = {
            let mut store = self.store.lock();
            self.bindings
                .omnifs_provider_browse()
                .call_list_children(&mut *store, id, path)?
        };

        let result = self.drive_effects(id, response).await?;

        // Intercept DirEntries to extract and cache projected files.
        if let wit_types::ActionResult::DirEntries(ref listing) = result {
            self.extract_projected_files(path, &listing.entries, listing.exhaustive);
        }

        // Strip projected_files before returning to FUSE.
        Ok(Self::strip_projected_files(result))
    }

    pub async fn call_read_file(
        &self,
        path: &str,
    ) -> Result<wit_types::ActionResult, RuntimeError> {
        let id = self.correlations.allocate();
        self.correlations.mark_pending(id, "read_file".into());

        let response = {
            let mut store = self.store.lock();
            self.bindings
                .omnifs_provider_browse()
                .call_read_file(&mut *store, id, path)?
        };

        self.drive_effects(id, response).await
    }

    pub async fn call_open_file(
        &self,
        path: &str,
    ) -> Result<wit_types::ActionResult, RuntimeError> {
        let id = self.correlations.allocate();
        self.correlations.mark_pending(id, "open_file".into());

        let response = {
            let mut store = self.store.lock();
            self.bindings
                .omnifs_provider_browse()
                .call_open_file(&mut *store, id, path)?
        };

        self.drive_effects(id, response).await
    }

    pub async fn call_read_chunk(
        &self,
        handle: u64,
        offset: u64,
        len: u32,
    ) -> Result<wit_types::ActionResult, RuntimeError> {
        let id = self.correlations.allocate();
        self.correlations.mark_pending(id, "read_chunk".into());

        let response = {
            let mut store = self.store.lock();
            self.bindings.omnifs_provider_browse().call_read_chunk(
                &mut *store,
                id,
                handle,
                offset,
                len,
            )?
        };

        self.drive_effects(id, response).await
    }

    pub fn call_close_file(&self, handle: u64) -> Result<(), RuntimeError> {
        let mut store = self.store.lock();
        self.bindings
            .omnifs_provider_browse()
            .call_close_file(&mut *store, handle)?;
        Ok(())
    }

    pub fn cache_get(&self, path: &str, kind: RecordKind) -> Option<CacheRecord> {
        self.l2.as_ref()?.get(path, kind).ok().flatten()
    }

    pub fn cache_put(&self, path: &str, kind: RecordKind, record: &CacheRecord) {
        if let Some(ref l2) = self.l2
            && let Err(e) = l2.put(path, kind, record)
        {
            tracing::debug!(path, error = %e, "L2 cache put failed");
        }
    }

    pub fn cache_put_batch(&self, records: &[(String, RecordKind, CacheRecord)]) {
        if let Some(ref l2) = self.l2
            && let Err(e) = l2.put_batch(records)
        {
            tracing::debug!(error = %e, "L2 cache batch put failed");
        }
    }

    pub fn cache_delete_prefix(&self, prefix: &str) {
        if let Some(ref l2) = self.l2
            && let Err(e) = l2.delete_prefix(prefix)
        {
            tracing::debug!(prefix, error = %e, "L2 cache prefix delete failed");
        }
    }

    /// Drain and return pending invalidated prefixes. Called by `FuseFs`
    /// before checking L0 to ensure stale entries are evicted.
    pub fn drain_invalidated_prefixes(&self) -> Vec<String> {
        let mut prefixes = self.invalidated_prefixes.lock();
        std::mem::take(&mut *prefixes)
    }

    /// Extract projected files from `DirEntries` and batch-write to L2.
    fn extract_projected_files(
        &self,
        parent_path: &str,
        entries: &[wit_types::DirEntry],
        exhaustive: bool,
    ) {
        use crate::cache::{
            AttrPayload, CacheRecord, DirentRecord, DirentsPayload, EntryKindCache, LookupPayload,
            RecordKind, ttl,
        };

        let mut batch = Vec::new();

        // Cache dirents for the parent directory.
        let dirent_records: Vec<DirentRecord> = entries
            .iter()
            .map(|e| DirentRecord {
                name: e.name.clone(),
                kind: match e.kind {
                    wit_types::EntryKind::Directory => EntryKindCache::Directory,
                    wit_types::EntryKind::File => EntryKindCache::File,
                },
                size: e.size.unwrap_or(0),
            })
            .collect();
        let dirents_payload = DirentsPayload {
            entries: dirent_records,
            exhaustive,
        };
        batch.push((
            parent_path.to_string(),
            RecordKind::Dirents,
            CacheRecord::new(
                RecordKind::Dirents,
                ttl::DIRENTS,
                dirents_payload.serialize(),
            ),
        ));

        for entry in entries {
            let child_path = if parent_path.is_empty() {
                entry.name.clone()
            } else {
                format!("{parent_path}/{}", entry.name)
            };

            let kind_cache = match entry.kind {
                wit_types::EntryKind::Directory => EntryKindCache::Directory,
                wit_types::EntryKind::File => EntryKindCache::File,
            };
            let size = entry.size.unwrap_or(0);

            // Cache lookup record for child.
            let lookup = LookupPayload::Positive {
                kind: kind_cache,
                size,
            };
            batch.push((
                child_path.clone(),
                RecordKind::Lookup,
                CacheRecord::new(RecordKind::Lookup, ttl::LOOKUP_POSITIVE, lookup.serialize()),
            ));

            // Cache attr record for child.
            let attr = AttrPayload {
                kind: kind_cache,
                size,
            };
            batch.push((
                child_path.clone(),
                RecordKind::Attr,
                CacheRecord::new(RecordKind::Attr, ttl::ATTR, attr.serialize()),
            ));

            // Cache projected files.
            if let Some(ref projected) = entry.projected_files {
                for pf in projected {
                    let file_path = format!("{child_path}/{}", pf.name);
                    let file_size = u64::try_from(pf.content.len()).unwrap_or(u64::MAX);

                    // File content record.
                    batch.push((
                        file_path.clone(),
                        RecordKind::File,
                        CacheRecord::new(RecordKind::File, ttl::PROJECTED_FILE, pf.content.clone()),
                    ));

                    // Lookup record for the projected file.
                    let pf_lookup = LookupPayload::Positive {
                        kind: EntryKindCache::File,
                        size: file_size,
                    };
                    batch.push((
                        file_path.clone(),
                        RecordKind::Lookup,
                        CacheRecord::new(
                            RecordKind::Lookup,
                            ttl::LOOKUP_POSITIVE,
                            pf_lookup.serialize(),
                        ),
                    ));

                    // Attr record for the projected file.
                    let pf_attr = AttrPayload {
                        kind: EntryKindCache::File,
                        size: file_size,
                    };
                    batch.push((
                        file_path,
                        RecordKind::Attr,
                        CacheRecord::new(RecordKind::Attr, ttl::ATTR, pf_attr.serialize()),
                    ));
                }
            }
        }

        if !batch.is_empty() {
            tracing::debug!(target: "omnifs_cache", kind = "prematerialize", count = batch.len(), "projected files extracted to L2");
            self.cache_put_batch(&batch);
        }
    }

    /// Strip `projected_files` from `DirEntries` before handing to FUSE.
    fn strip_projected_files(result: wit_types::ActionResult) -> wit_types::ActionResult {
        if let wit_types::ActionResult::DirEntries(listing) = result {
            let stripped: Vec<wit_types::DirEntry> = listing
                .entries
                .into_iter()
                .map(|mut e| {
                    e.projected_files = None;
                    e
                })
                .collect();
            wit_types::ActionResult::DirEntries(DirListing {
                entries: stripped,
                exhaustive: listing.exhaustive,
            })
        } else {
            result
        }
    }

    pub async fn call_timer_tick(&self) -> Result<wit_types::ActionResult, RuntimeError> {
        let id = self.correlations.allocate();
        self.correlations.mark_pending(id, "timer_tick".into());

        let response = {
            let mut store = self.store.lock();
            self.bindings.omnifs_provider_notify().call_on_event(
                &mut *store,
                id,
                &wit_types::ProviderEvent::TimerTick,
            )?
        };

        self.drive_effects(id, response).await
    }

    async fn drive_effects(
        &self,
        id: u64,
        response: wit_types::ProviderResponse,
    ) -> Result<wit_types::ActionResult, RuntimeError> {
        let result = self.drive_effects_inner(id, response).await;
        // Always clean up the correlation, whether we succeeded or hit an error.
        self.correlations.resolve(id);
        result
    }

    async fn drive_effects_inner(
        &self,
        id: u64,
        mut response: wit_types::ProviderResponse,
    ) -> Result<wit_types::ActionResult, RuntimeError> {
        loop {
            match response {
                wit_types::ProviderResponse::Done(result) => {
                    return Ok(result);
                }
                wit_types::ProviderResponse::Effect(effect) => {
                    let result = self.execute_single_effect(&effect).await;
                    let effect_result = wit_types::EffectResult::Single(result);
                    let mut store = self.store.lock();
                    response = self.bindings.omnifs_provider_resume().call_resume(
                        &mut *store,
                        id,
                        &effect_result,
                    )?;
                }
                wit_types::ProviderResponse::Batch(effects) => {
                    let results = self.execute_batch(&effects).await;
                    let effect_result = wit_types::EffectResult::Batch(results);
                    let mut store = self.store.lock();
                    response = self.bindings.omnifs_provider_resume().call_resume(
                        &mut *store,
                        id,
                        &effect_result,
                    )?;
                }
            }
        }
    }

    /// Resolve a tree-ref handle to a real filesystem path.
    /// Returns the clone directory for the given handle.
    pub fn resolve_tree_ref(&self, tree_ref: u64) -> Option<std::path::PathBuf> {
        self.git.repo_path(tree_ref)
    }

    fn resolve_response_sync(
        response: wit_types::ProviderResponse,
    ) -> Result<wit_types::ActionResult, RuntimeError> {
        match response {
            wit_types::ProviderResponse::Done(result) => Ok(result),
            _ => Err(RuntimeError::ProviderError(
                "initialize must not return effects".into(),
            )),
        }
    }

    async fn execute_single_effect(
        &self,
        effect: &wit_types::SingleEffect,
    ) -> wit_types::SingleEffectResult {
        match effect {
            wit_types::SingleEffect::Fetch(req) => {
                let headers: Vec<(String, String)> = req
                    .headers
                    .iter()
                    .map(|h| (h.name.clone(), h.value.clone()))
                    .collect();
                let resp = self
                    .http
                    .execute_fetch(&req.method, &req.url, &headers, req.body.as_deref())
                    .await;
                effect_response_to_wit(resp)
            }
            wit_types::SingleEffect::KvGet(key) => match self.kv.get(key) {
                Some(val) => wit_types::SingleEffectResult::KvValue(Some(val)),
                None => wit_types::SingleEffectResult::KvValue(None),
            },
            wit_types::SingleEffect::KvSet(req) => {
                self.kv.set(&req.key, req.value.clone());
                wit_types::SingleEffectResult::KvOk
            }
            wit_types::SingleEffect::KvDelete(key) => {
                self.kv.delete(key);
                wit_types::SingleEffectResult::KvOk
            }
            wit_types::SingleEffect::KvListKeys(prefix) => {
                let keys = self.kv.list_keys(prefix);
                wit_types::SingleEffectResult::KvKeys(keys)
            }
            wit_types::SingleEffect::GitOpenRepo(req) => {
                git_response_to_wit(self.git.open_repo(&req.cache_key, &req.clone_url))
            }
            wit_types::SingleEffect::GitListTree(req) => {
                git_response_to_wit(self.git.list_tree(req.repo, &req.ref_name, &req.path))
            }
            wit_types::SingleEffect::GitReadBlob(req) => {
                git_response_to_wit(self.git.read_blob(req.repo, &req.oid))
            }
            wit_types::SingleEffect::GitHeadRef(repo_id) => {
                git_response_to_wit(self.git.head_ref(*repo_id))
            }
            wit_types::SingleEffect::GitListCachedRepos(req) => {
                git_response_to_wit(self.git.list_cached_repos(req.prefix.as_deref()))
            }
            wit_types::SingleEffect::CacheInvalidatePrefix(req) => {
                self.cache_delete_prefix(&req.prefix);
                self.invalidated_prefixes.lock().push(req.prefix.clone());
                wit_types::SingleEffectResult::CacheOk
            }
            _ => wit_types::SingleEffectResult::EffectError(wit_types::EffectError {
                kind: wit_types::ErrorKind::Internal,
                message: "effect type not yet implemented".to_string(),
                retryable: false,
            }),
        }
    }

    async fn execute_batch(
        &self,
        effects: &[wit_types::SingleEffect],
    ) -> Vec<wit_types::SingleEffectResult> {
        let futures: Vec<_> = effects
            .iter()
            .map(|effect| self.execute_single_effect(effect))
            .collect();
        futures::future::join_all(futures).await
    }
}

fn build_grants(config: &InstanceConfig, needs_git: bool) -> CapabilityGrants {
    let caps = config.capabilities.as_ref();
    CapabilityGrants {
        domains: caps.and_then(|c| c.domains.clone()).unwrap_or_default(),
        git_repos: caps.and_then(|c| c.git_repos.clone()).unwrap_or_default(),
        max_memory_mb: caps.and_then(|c| c.max_memory_mb).unwrap_or(64),
        needs_git,
    }
}

fn validate_instance_config(wit_schema: &wit_types::ConfigSchema, config: &InstanceConfig) {
    let schema_fields: Vec<SchemaField> = wit_schema
        .fields
        .iter()
        .map(|f| SchemaField {
            name: f.name.clone(),
            field_type: f.field_type.clone(),
            required: f.required,
            default_value: f.default_value.clone(),
            description: f.description.clone(),
        })
        .collect();

    let config_value = config
        .config_raw
        .clone()
        .unwrap_or(toml::Value::Table(toml::map::Map::new()));

    if let Err(e) = schema::validate_config(&schema_fields, &config_value) {
        tracing::warn!("config validation: {e}");
    }
}

fn error_kind_to_wit(kind: ErrorKind) -> wit_types::ErrorKind {
    match kind {
        ErrorKind::Network => wit_types::ErrorKind::Network,
        ErrorKind::Timeout => wit_types::ErrorKind::Timeout,
        ErrorKind::Denied => wit_types::ErrorKind::Denied,
        ErrorKind::NotFound => wit_types::ErrorKind::NotFound,
        ErrorKind::RateLimited => wit_types::ErrorKind::RateLimited,
        ErrorKind::Internal => wit_types::ErrorKind::Internal,
    }
}

fn git_entry_kind_to_wit(kind: GitEntryKind) -> wit_types::GitEntryKind {
    match kind {
        GitEntryKind::Blob => wit_types::GitEntryKind::Blob,
        GitEntryKind::Tree => wit_types::GitEntryKind::Tree,
        GitEntryKind::Commit => wit_types::GitEntryKind::Commit,
    }
}

fn effect_response_to_wit(resp: EffectResponse) -> wit_types::SingleEffectResult {
    match resp {
        EffectResponse::HttpResponse {
            status,
            headers,
            body,
        } => wit_types::SingleEffectResult::HttpResponse(wit_types::HttpResponse {
            status,
            headers: headers
                .into_iter()
                .map(|(name, value)| wit_types::Header { name, value })
                .collect(),
            body,
        }),
        EffectResponse::Error {
            kind,
            message,
            retryable,
        } => wit_types::SingleEffectResult::EffectError(wit_types::EffectError {
            kind: error_kind_to_wit(kind),
            message,
            retryable,
        }),
        _ => wit_types::SingleEffectResult::EffectError(wit_types::EffectError {
            kind: wit_types::ErrorKind::Internal,
            message: "unexpected effect response type".to_string(),
            retryable: false,
        }),
    }
}

fn git_response_to_wit(resp: EffectResponse) -> wit_types::SingleEffectResult {
    match resp {
        EffectResponse::GitRepoOpened(id) => {
            wit_types::SingleEffectResult::GitRepoOpened(wit_types::GitRepoInfo {
                repo: id,
                tree: id,
            })
        }
        EffectResponse::GitTreeEntries(entries) => wit_types::SingleEffectResult::GitTreeEntries(
            entries
                .into_iter()
                .map(|e| wit_types::GitTreeEntry {
                    name: e.name,
                    mode: e.mode,
                    oid: e.oid,
                    kind: git_entry_kind_to_wit(e.kind),
                })
                .collect(),
        ),
        EffectResponse::GitBlobData(data) => wit_types::SingleEffectResult::GitBlobData(data),
        EffectResponse::GitRef(ref_name) => wit_types::SingleEffectResult::GitRef(ref_name),
        EffectResponse::GitCachedRepos(repos) => wit_types::SingleEffectResult::GitCachedRepos(
            repos
                .into_iter()
                .map(|repo| wit_types::GitCachedRepo {
                    cache_key: repo.cache_key,
                })
                .collect(),
        ),
        EffectResponse::Error {
            kind,
            message,
            retryable,
        } => wit_types::SingleEffectResult::EffectError(wit_types::EffectError {
            kind: error_kind_to_wit(kind),
            message,
            retryable,
        }),
        _ => wit_types::SingleEffectResult::EffectError(wit_types::EffectError {
            kind: wit_types::ErrorKind::Internal,
            message: "unexpected git response type".to_string(),
            retryable: false,
        }),
    }
}
