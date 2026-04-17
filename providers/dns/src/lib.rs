use omnifs_sdk::prelude::*;

mod browse;
mod doh;
pub(crate) mod types;

use types::{DomainName, RecordType, Segment};

pub(crate) struct State {
    pub resolvers: doh::ResolverConfig,
}

pub(crate) enum Continuation {
    Single,
    All { results: Vec<DnsRecord> },
    Raw { domain: String },
}

#[derive(Clone, Debug)]
pub(crate) struct DnsRecord {
    pub rtype: RecordType,
    pub value: String,
}

#[omnifs_sdk::config]
struct Config {
    #[serde(default = "default_resolver_name")]
    default_resolver: String,
    #[serde(default)]
    resolvers: std::collections::BTreeMap<String, ConfigResolver>,
}

fn default_resolver_name() -> String {
    String::from("cloudflare")
}

#[omnifs_sdk::config]
struct ConfigResolver {
    url: String,
    #[serde(default)]
    aliases: Vec<String>,
}

#[allow(clippy::unnecessary_wraps)]
#[omnifs_sdk::provider]
impl DnsProvider {
    fn init(config: Config) -> (State, ProviderInfo) {
        let resolvers = doh::ResolverConfig::from_config(config.default_resolver, config.resolvers);
        (
            State { resolvers },
            ProviderInfo {
                name: "dns-provider".to_string(),
                version: "0.1.0".to_string(),
                description: "DNS record browsing via DNS-over-HTTPS".to_string(),
            },
        )
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["cloudflare-dns.com".to_string(), "dns.google".to_string()],
            auth_types: vec![],
            max_memory_mb: 32,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 0,
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    fn resume(id: u64, cont: Continuation, outcome: EffectResult) -> ProviderResponse {
        browse::resume(id, cont, outcome)
    }

    #[route("/")]
    fn root(op: Op) -> Option<ProviderResponse> {
        match op {
            Op::Lookup(_) => None,
            Op::List(_) => {
                let mut entries = vec![mk_file("_resolvers"), mk_dir("_reverse")];
                for name in browse::resolver_dir_names() {
                    entries.push(mk_dir(name));
                }
                Some(ProviderResponse::Done(ActionResult::DirEntries(
                    DirListing {
                        entries,
                        exhaustive: false,
                    },
                )))
            }
            Op::Read(_) => Some(err(ProviderError::invalid_input("not a file"))),
        }
    }

    #[allow(clippy::unnecessary_wraps)]
    #[route("/_resolvers")]
    fn resolvers_file(op: Op) -> Option<ProviderResponse> {
        file_only(op, "_resolvers", |_| browse::resolvers_content())
    }

    #[allow(clippy::unnecessary_wraps)]
    #[route("/_reverse")]
    fn reverse_root(op: Op) -> Option<ProviderResponse> {
        dir_only(op, "_reverse", |_| {
            ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                entries: vec![],
                exhaustive: false,
            }))
        })
    }

    #[allow(clippy::unnecessary_wraps)]
    #[route("/_reverse/{ip}")]
    fn reverse_ip(op: Op, ip: &str) -> Option<ProviderResponse> {
        file_only(op, ip, |id| Self::read_reverse_ip(id, None, ip))
    }

    #[allow(clippy::unnecessary_wraps)]
    #[route("/@{segment}")]
    fn resolver_root(op: Op, segment: &str) -> Option<ProviderResponse> {
        let segment = format!("@{segment}");

        dir_only(op, segment, |_| {
            ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                entries: vec![],
                exhaustive: false,
            }))
        })
    }

    #[route("/{segment}")]
    fn segment(op: Op, segment: Segment) -> Option<ProviderResponse> {
        match segment {
            Segment::Ip(ip) => {
                let segment = ip.to_string();
                file_only(op, &segment, |id| Self::read_reverse_ip(id, None, &segment))
            }
            Segment::Domain(domain) => {
                let segment = domain.as_ref();
                dir_only(op, segment, |_| Self::list_domain())
            }
        }
    }

    #[route("/@{resolver}/{segment}")]
    fn resolver_segment(op: Op, resolver: &str, segment: Segment) -> Option<ProviderResponse> {
        match segment {
            Segment::Ip(ip) => {
                let segment = ip.to_string();
                file_only(op, &segment, |id| {
                    Self::read_reverse_ip(id, Some(resolver), &segment)
                })
            }
            Segment::Domain(domain) => {
                let segment = domain.as_ref();
                dir_only(op, segment, |_| Self::list_domain())
            }
        }
    }

    #[route("/{domain}/{record}")]
    fn domain_record(op: Op, domain: DomainName, record: &str) -> Option<ProviderResponse> {
        let domain = domain.as_ref();
        Self::record_handler(op, None, domain, record)
    }

    #[route("/@{resolver}/{domain}/{record}")]
    fn resolver_domain_record(
        op: Op,
        resolver: &str,
        domain: DomainName,
        record: &str,
    ) -> Option<ProviderResponse> {
        let domain = domain.as_ref();
        Self::record_handler(op, Some(resolver), domain, record)
    }

    fn list_domain() -> ProviderResponse {
        let mut entries: Vec<DirEntry> = RecordType::all()
            .iter()
            .map(|rt| mk_file(rt.as_ref()))
            .collect();
        entries.push(mk_file("_all"));
        entries.push(mk_file("_raw"));
        ProviderResponse::Done(ActionResult::DirEntries(DirListing {
            entries,
            exhaustive: true,
        }))
    }

    fn record_handler(
        op: Op,
        resolver: Option<&str>,
        domain: &str,
        record_name: &str,
    ) -> Option<ProviderResponse> {
        match record_name {
            "_all" => file_only(op, "_all", |id| Self::read_all(id, resolver, domain)),
            "_raw" => file_only(op, "_raw", |id| Self::read_raw(id, resolver, domain)),
            "PTR" => None,
            _ => {
                let rtype = record_name.parse::<RecordType>().ok()?;
                file_only(op, record_name, |id| {
                    Self::read_record(id, resolver, domain, rtype)
                })
            }
        }
    }

    fn read_record(
        id: u64,
        resolver: Option<&str>,
        domain: &str,
        rtype: RecordType,
    ) -> ProviderResponse {
        let effect = match with_state(|s| doh::query(&s.resolvers, resolver, domain, rtype)) {
            Ok(e) => e,
            Err(e) => return err(ProviderError::internal(e)),
        };
        dispatch(id, Continuation::Single, effect)
    }

    fn read_all(id: u64, resolver: Option<&str>, domain: &str) -> ProviderResponse {
        let types = RecordType::common();
        let effects = match with_state(|s| {
            types
                .iter()
                .map(|&rt| doh::query(&s.resolvers, resolver, domain, rt))
                .collect()
        }) {
            Ok(e) => e,
            Err(e) => return err(ProviderError::internal(e)),
        };
        dispatch_batch(
            id,
            Continuation::All {
                results: Vec::new(),
            },
            effects,
        )
    }

    fn read_raw(id: u64, resolver: Option<&str>, domain: &str) -> ProviderResponse {
        let effect = match with_state(|s| doh::query(&s.resolvers, resolver, domain, RecordType::A))
        {
            Ok(e) => e,
            Err(e) => return err(ProviderError::internal(e)),
        };
        dispatch(
            id,
            Continuation::Raw {
                domain: domain.to_string(),
            },
            effect,
        )
    }

    fn read_reverse_ip(id: u64, resolver: Option<&str>, ip: &str) -> ProviderResponse {
        let effect = match with_state(|s| doh::reverse_query(&s.resolvers, resolver, ip)) {
            Ok(Ok(effect)) => effect,
            Ok(Err(e)) => return err(e),
            Err(e) => return err(ProviderError::internal(e)),
        };
        dispatch(id, Continuation::Single, effect)
    }
}
