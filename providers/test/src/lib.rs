wit_bindgen::generate!({
    path: "../../wit",
    world: "provider",
});

#[allow(dead_code)]
type ProviderResult<T> = Result<T, String>;

struct TestProvider;

impl exports::omnifs::provider::lifecycle::Guest for TestProvider {
    fn initialize(_config: Vec<u8>) -> omnifs::provider::types::ProviderResponse {
        use omnifs::provider::types::*;
        ProviderResponse::Done(ActionResult::ProviderInitialized(ProviderInfo {
            name: "test-provider".to_string(),
            version: "0.1.0".to_string(),
            description: "A test provider with canned data".to_string(),
        }))
    }

    fn shutdown() {}

    fn get_config_schema() -> omnifs::provider::types::ConfigSchema {
        omnifs::provider::types::ConfigSchema { fields: vec![] }
    }

    fn capabilities() -> omnifs::provider::types::RequestedCapabilities {
        omnifs::provider::types::RequestedCapabilities {
            domains: vec!["httpbin.org".to_string()],
            auth_types: vec![],
            max_memory_mb: 16,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 0,
        }
    }
}

impl exports::omnifs::provider::browse::Guest for TestProvider {
    fn resolve_entry(
        _id: u64,
        parent_path: String,
        name: String,
    ) -> omnifs::provider::types::ProviderResponse {
        use omnifs::provider::types::*;
        let path = if parent_path.is_empty() {
            name.clone()
        } else {
            format!("{parent_path}/{name}")
        };
        match path.as_str() {
            "hello" => ProviderResponse::Done(ActionResult::DirEntryOption(Some(DirEntry {
                name: "hello".to_string(),
                kind: EntryKind::Directory,
                size: None,
                projected_files: None,
            }))),
            "hello/message" => {
                ProviderResponse::Done(ActionResult::DirEntryOption(Some(DirEntry {
                    name: "message".to_string(),
                    kind: EntryKind::File,
                    size: Some(13),
                    projected_files: None,
                })))
            }
            "hello/greeting" => {
                ProviderResponse::Done(ActionResult::DirEntryOption(Some(DirEntry {
                    name: "greeting".to_string(),
                    kind: EntryKind::File,
                    size: Some(12),
                    projected_files: None,
                })))
            }
            "hello/cached" => {
                ProviderResponse::Done(ActionResult::DirEntryOption(Some(DirEntry {
                    name: "cached".to_string(),
                    kind: EntryKind::File,
                    size: None,
                    projected_files: None,
                })))
            }
            _ => ProviderResponse::Done(ActionResult::DirEntryOption(None)),
        }
    }

    fn list_entries(_id: u64, path: String) -> omnifs::provider::types::ProviderResponse {
        use omnifs::provider::types::*;
        match path.as_str() {
            "" => ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                entries: vec![DirEntry {
                    name: "hello".to_string(),
                    kind: EntryKind::Directory,
                    size: None,
                    projected_files: None,
                }],
                exhaustive: true,
            })),
            "hello" => ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                entries: vec![
                    DirEntry {
                        name: "message".to_string(),
                        kind: EntryKind::File,
                        size: Some(13),
                        projected_files: None,
                    },
                    DirEntry {
                        name: "greeting".to_string(),
                        kind: EntryKind::File,
                        size: Some(12),
                        projected_files: None,
                    },
                ],
                exhaustive: true,
            })),
            _ => ProviderResponse::Done(ActionResult::Err("not found".to_string())),
        }
    }

    fn read_file(_id: u64, path: String) -> omnifs::provider::types::ProviderResponse {
        use omnifs::provider::types::*;
        match path.as_str() {
            "hello/message" => {
                ProviderResponse::Done(ActionResult::FileContent(b"Hello, world!".to_vec()))
            }
            "hello/greeting" => {
                ProviderResponse::Done(ActionResult::FileContent(b"Hi there!\n".to_vec()))
            }
            // Special path that issues a KV-set then KV-get effect chain to
            // exercise the host's effect/resume loop.
            "hello/cached" => ProviderResponse::Effect(SingleEffect::KvSet(KvSetRequest {
                key: "test:cached".to_string(),
                value: b"cached-value".to_vec(),
            })),
            _ => ProviderResponse::Done(ActionResult::Err("not found".to_string())),
        }
    }

    fn open_file(_id: u64, _path: String) -> omnifs::provider::types::ProviderResponse {
        omnifs::provider::types::ProviderResponse::Done(
            omnifs::provider::types::ActionResult::FileOpened(1),
        )
    }

    fn read_chunk(
        _id: u64,
        _handle: u64,
        _offset: u64,
        _len: u32,
    ) -> omnifs::provider::types::ProviderResponse {
        omnifs::provider::types::ProviderResponse::Done(
            omnifs::provider::types::ActionResult::FileChunk(vec![]),
        )
    }

    fn close_file(_handle: u64) {}
}

impl exports::omnifs::provider::resume::Guest for TestProvider {
    fn resume(
        _id: u64,
        effect_outcome: omnifs::provider::types::EffectResult,
    ) -> omnifs::provider::types::ProviderResponse {
        use omnifs::provider::types::*;

        let result = match &effect_outcome {
            EffectResult::Single(r) => r,
            EffectResult::Batch(v) if !v.is_empty() => &v[0],
            EffectResult::Batch(_) => {
                return ProviderResponse::Done(ActionResult::Err(
                    "unexpected batch result".to_string(),
                ));
            }
        };

        match result {
            // After KV-set completes, issue a KV-get to read it back.
            SingleEffectResult::KvOk => {
                ProviderResponse::Effect(SingleEffect::KvGet("test:cached".to_string()))
            }
            // After KV-get completes, return the value as file content.
            SingleEffectResult::KvValue(Some(data)) => {
                ProviderResponse::Done(ActionResult::FileContent(data.clone()))
            }
            SingleEffectResult::KvValue(None) => {
                ProviderResponse::Done(ActionResult::FileContent(b"no-cached-value".to_vec()))
            }
            _ => ProviderResponse::Done(ActionResult::Err("unexpected resume".to_string())),
        }
    }

    fn cancel(_id: u64) {}
}

impl exports::omnifs::provider::reconcile::Guest for TestProvider {
    fn plan_mutations(
        _id: u64,
        _changes: Vec<omnifs::provider::types::FileChange>,
    ) -> omnifs::provider::types::ProviderResponse {
        use omnifs::provider::types::*;
        ProviderResponse::Done(ActionResult::Err(
            "mutations are not implemented".to_string(),
        ))
    }

    fn execute(
        _id: u64,
        _mutation: omnifs::provider::types::PlannedMutation,
    ) -> omnifs::provider::types::ProviderResponse {
        use omnifs::provider::types::*;
        ProviderResponse::Done(ActionResult::Err(
            "mutations are not implemented".to_string(),
        ))
    }

    fn fetch_resource(
        _id: u64,
        _resource_path: String,
    ) -> omnifs::provider::types::ProviderResponse {
        use omnifs::provider::types::*;
        ProviderResponse::Done(ActionResult::Err(
            "fetch_resource is not implemented".to_string(),
        ))
    }

    fn list_scope(_id: u64, _scope: String) -> omnifs::provider::types::ProviderResponse {
        use omnifs::provider::types::*;
        ProviderResponse::Done(ActionResult::Err(
            "list_scope is not implemented".to_string(),
        ))
    }
}

impl exports::omnifs::provider::notify::Guest for TestProvider {
    fn on_event(
        _id: u64,
        _event: omnifs::provider::types::ProviderEvent,
    ) -> omnifs::provider::types::ProviderResponse {
        use omnifs::provider::types::*;
        ProviderResponse::Done(ActionResult::Ok)
    }
}

export!(TestProvider);
