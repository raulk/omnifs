use omnifs_sdk::prelude::*;

#[derive(Deserialize)]
struct Config {}

struct State;

enum Continuation {
    AwaitingKvSet,
    AwaitingKvGet,
}

#[omnifs_sdk::provider]
impl TestProvider {
    fn init(_config: Config) -> (State, ProviderInfo) {
        (
            State,
            ProviderInfo {
                name: "test-provider".into(),
                version: "0.1.0".into(),
                description: "A test provider with canned data".into(),
            },
        )
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["httpbin.org".into()],
            auth_types: vec![],
            max_memory_mb: 16,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 0,
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    fn resume(id: u64, cont: Continuation, outcome: EffectResult) -> ProviderResponse {
        let result = match &outcome {
            EffectResult::Single(r) => r,
            EffectResult::Batch(v) if !v.is_empty() => &v[0],
            EffectResult::Batch(_) => return err("unexpected batch result"),
        };
        match cont {
            Continuation::AwaitingKvSet => match result {
                SingleEffectResult::KvOk => dispatch(
                    id,
                    Continuation::AwaitingKvGet,
                    SingleEffect::KvGet("test:cached".into()),
                ),
                _ => err("expected KvOk"),
            },
            Continuation::AwaitingKvGet => match result {
                SingleEffectResult::KvValue(Some(data)) => {
                    ProviderResponse::Done(ActionResult::FileContent(data.clone()))
                }
                SingleEffectResult::KvValue(None) => {
                    ProviderResponse::Done(ActionResult::FileContent(b"no-cached-value".to_vec()))
                }
                _ => err("expected KvValue"),
            },
        }
    }

    #[route("/")]
    fn root(op: Op) -> Option<ProviderResponse> {
        match op {
            Op::List(_) => Some(ProviderResponse::Done(ActionResult::DirEntries(
                DirListing {
                    entries: vec![mk_dir("hello")],
                    exhaustive: true,
                },
            ))),
            Op::Lookup(_) | Op::Read(_) => None,
        }
    }

    #[route("/hello")]
    fn hello(op: Op) -> Option<ProviderResponse> {
        match op {
            Op::Lookup(_) => Some(dir_entry("hello")),
            Op::List(_) => Some(ProviderResponse::Done(ActionResult::DirEntries(
                DirListing {
                    entries: vec![
                        DirEntry {
                            name: "message".into(),
                            kind: EntryKind::File,
                            size: Some(13),
                            projected_files: None,
                        },
                        DirEntry {
                            name: "greeting".into(),
                            kind: EntryKind::File,
                            size: Some(12),
                            projected_files: None,
                        },
                    ],
                    exhaustive: true,
                },
            ))),
            Op::Read(_) => None,
        }
    }

    #[route("/hello/message")]
    fn message(op: Op) -> Option<ProviderResponse> {
        match op {
            Op::Lookup(_) => Some(ProviderResponse::Done(ActionResult::DirEntryOption(Some(
                DirEntry {
                    name: "message".into(),
                    kind: EntryKind::File,
                    size: Some(13),
                    projected_files: None,
                },
            )))),
            Op::Read(_) => Some(ProviderResponse::Done(ActionResult::FileContent(
                b"Hello, world!".to_vec(),
            ))),
            Op::List(_) => None,
        }
    }

    #[route("/hello/greeting")]
    fn greeting(op: Op) -> Option<ProviderResponse> {
        match op {
            Op::Lookup(_) => Some(ProviderResponse::Done(ActionResult::DirEntryOption(Some(
                DirEntry {
                    name: "greeting".into(),
                    kind: EntryKind::File,
                    size: Some(12),
                    projected_files: None,
                },
            )))),
            Op::Read(_) => Some(ProviderResponse::Done(ActionResult::FileContent(
                b"Hi there!\n".to_vec(),
            ))),
            Op::List(_) => None,
        }
    }

    #[route("/hello/cached")]
    fn cached(op: Op) -> Option<ProviderResponse> {
        match op {
            Op::Lookup(_) => Some(ProviderResponse::Done(ActionResult::DirEntryOption(Some(
                DirEntry {
                    name: "cached".into(),
                    kind: EntryKind::File,
                    size: None,
                    projected_files: None,
                },
            )))),
            Op::Read(id) => Some(dispatch(
                id,
                Continuation::AwaitingKvSet,
                SingleEffect::KvSet(KvSetRequest {
                    key: "test:cached".into(),
                    value: b"cached-value".to_vec(),
                }),
            )),
            Op::List(_) => None,
        }
    }
}
