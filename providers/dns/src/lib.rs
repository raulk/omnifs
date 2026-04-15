wit_bindgen::generate!({
    path: "../../wit",
    world: "provider",
});

mod browse;
mod doh;
pub(crate) mod path;

use hashbrown::HashMap;
use omnifs::provider::types::*;
use path::RecordType;
use std::cell::RefCell;

struct DnsProvider;

thread_local! {
    static STATE: RefCell<Option<ProviderState>> = const { RefCell::new(None) };
}

pub(crate) struct ProviderState {
    pending: HashMap<u64, Continuation>,
    pub resolvers: doh::ResolverConfig,
}

pub(crate) struct QueryContext {
    #[allow(dead_code)]
    pub resolver: Option<String>,
    pub domain: String,
}

#[allow(dead_code)]
enum Continuation {
    Single { ctx: QueryContext, rtype: RecordType },
    All { ctx: QueryContext, results: Vec<DnsRecord>, pending_types: Vec<RecordType> },
    Raw { ctx: QueryContext },
}

#[derive(Clone, Debug)]
pub(crate) struct DnsRecord {
    pub rtype: RecordType,
    pub value: String,
}

fn with_state<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut ProviderState) -> R,
{
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        match borrow.as_mut() {
            Some(state) => Ok(f(state)),
            None => Err("provider not initialized".to_string()),
        }
    })
}

impl exports::omnifs::provider::lifecycle::Guest for DnsProvider {
    fn initialize(config: Vec<u8>) -> ProviderResponse {
        let resolvers = doh::ResolverConfig::from_toml(&config);
        STATE.with(|s| {
            *s.borrow_mut() = Some(ProviderState {
                pending: HashMap::new(),
                resolvers,
            });
        });
        ProviderResponse::Done(ActionResult::ProviderInitialized(ProviderInfo {
            name: "dns-provider".to_string(),
            version: "0.1.0".to_string(),
            description: "DNS record browsing via DNS-over-HTTPS".to_string(),
        }))
    }

    fn shutdown() {
        STATE.with(|s| *s.borrow_mut() = None);
    }

    fn get_config_schema() -> ConfigSchema {
        ConfigSchema {
            fields: vec![
                ConfigField {
                    name: "default_resolver".to_string(),
                    field_type: "string".to_string(),
                    required: false,
                    default_value: Some("cloudflare".to_string()),
                    description: "Default resolver alias used when no @resolver prefix"
                        .to_string(),
                },
                ConfigField {
                    name: "resolvers".to_string(),
                    field_type: "table".to_string(),
                    required: false,
                    default_value: None,
                    description: "Named resolver aliases mapping to DoH endpoint URLs".to_string(),
                },
            ],
        }
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec![
                "cloudflare-dns.com".to_string(),
                "dns.google".to_string(),
            ],
            auth_types: vec![],
            max_memory_mb: 32,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 0,
        }
    }
}

impl exports::omnifs::provider::browse::Guest for DnsProvider {
    fn lookup_child(id: u64, parent_path: String, name: String) -> ProviderResponse {
        browse::lookup_child(id, &parent_path, &name)
    }

    fn list_children(id: u64, path: String) -> ProviderResponse {
        browse::list_children(id, &path)
    }

    fn read_file(id: u64, path: String) -> ProviderResponse {
        browse::read_file(id, &path)
    }

    fn open_file(_id: u64, _path: String) -> ProviderResponse {
        ProviderResponse::Done(ActionResult::FileOpened(1))
    }

    fn read_chunk(_id: u64, _handle: u64, _offset: u64, _len: u32) -> ProviderResponse {
        ProviderResponse::Done(ActionResult::FileChunk(vec![]))
    }

    fn close_file(_handle: u64) {}
}

impl exports::omnifs::provider::resume::Guest for DnsProvider {
    fn resume(id: u64, effect_outcome: EffectResult) -> ProviderResponse {
        browse::resume(id, effect_outcome)
    }

    fn cancel(id: u64) {
        let _ = with_state(|s| { s.pending.remove(&id); });
    }
}

const NOT_IMPL: &str = "not implemented";

impl exports::omnifs::provider::reconcile::Guest for DnsProvider {
    fn plan_mutations(_id: u64, _changes: Vec<FileChange>) -> ProviderResponse {
        ProviderResponse::Done(ActionResult::Err(NOT_IMPL.to_string()))
    }
    fn execute(_id: u64, _mutation: PlannedMutation) -> ProviderResponse {
        ProviderResponse::Done(ActionResult::Err(NOT_IMPL.to_string()))
    }
    fn fetch_resource(_id: u64, _resource_path: String) -> ProviderResponse {
        ProviderResponse::Done(ActionResult::Err(NOT_IMPL.to_string()))
    }
    fn list_scope(_id: u64, _scope: String) -> ProviderResponse {
        ProviderResponse::Done(ActionResult::Err(NOT_IMPL.to_string()))
    }
}

impl exports::omnifs::provider::notify::Guest for DnsProvider {
    fn on_event(_id: u64, _event: ProviderEvent) -> ProviderResponse {
        ProviderResponse::Done(ActionResult::Ok)
    }
}

export!(DnsProvider);
