use omnifs_sdk::prelude::*;

mod browse;
mod doh;
pub(crate) mod types;

use types::{RecordType, is_domain_like, is_ip_addr};

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
    "cloudflare".to_string()
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

    // --- Routes ---

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
            Op::Read(_) => Some(err("not a file")),
        }
    }

    #[allow(clippy::unnecessary_wraps)]
    #[route("/_resolvers")]
    fn resolvers_file(op: Op) -> Option<ProviderResponse> {
        match op {
            Op::Lookup(_) => Some(file_entry("_resolvers")),
            Op::Read(_) => Some(browse::resolvers_content()),
            Op::List(_) => Some(err("not a directory")),
        }
    }

    #[allow(clippy::unnecessary_wraps)]
    #[route("/_reverse")]
    fn reverse_root(op: Op) -> Option<ProviderResponse> {
        match op {
            Op::Lookup(_) => Some(dir_entry("_reverse")),
            Op::List(_) => Some(ProviderResponse::Done(ActionResult::DirEntries(
                DirListing {
                    entries: vec![],
                    exhaustive: false,
                },
            ))),
            Op::Read(_) => Some(err("not a file")),
        }
    }

    #[allow(clippy::unnecessary_wraps)]
    #[route("/_reverse/{ip}")]
    fn reverse_ip(op: Op, ip: &str) -> Option<ProviderResponse> {
        match op {
            Op::Lookup(_) => Some(file_entry(ip)),
            Op::Read(id) => Some(Self::read_reverse_ip(id, None, ip)),
            Op::List(_) => Some(err("not a directory")),
        }
    }

    /// Single segment: could be @resolver, IP address, or domain.
    #[route("/{segment}")]
    fn single_segment(op: Op, segment: &str) -> Option<ProviderResponse> {
        if let Some(resolver) = segment.strip_prefix('@') {
            // @resolver directory
            let _ = resolver;
            return match op {
                Op::Lookup(_) => Some(dir_entry(segment)),
                Op::List(_) => Some(ProviderResponse::Done(ActionResult::DirEntries(
                    DirListing {
                        entries: vec![],
                        exhaustive: false,
                    },
                ))),
                Op::Read(_) => Some(err("not a file")),
            };
        }

        if is_ip_addr(segment) {
            // Bare IP: direct reverse lookup file
            return match op {
                Op::Lookup(_) => Some(file_entry(segment)),
                Op::Read(id) => Some(Self::read_reverse_ip(id, None, segment)),
                Op::List(_) => Some(err("not a directory")),
            };
        }

        if !is_domain_like(segment) {
            return None;
        }

        // Domain directory
        match op {
            Op::Lookup(_) => Some(dir_entry(segment)),
            Op::List(_) => Some(Self::list_domain()),
            Op::Read(_) => Some(err("not a file")),
        }
    }

    /// Two segments: @resolver/target or domain/record.
    #[route("/{first}/{second}")]
    fn two_segments(op: Op, first: &str, second: &str) -> Option<ProviderResponse> {
        if let Some(resolver) = first.strip_prefix('@') {
            // @resolver/<target>
            if is_ip_addr(second) {
                // @resolver/IP: reverse lookup file
                return match op {
                    Op::Lookup(_) => Some(file_entry(second)),
                    Op::Read(id) => Some(Self::read_reverse_ip(id, Some(resolver), second)),
                    Op::List(_) => Some(err("not a directory")),
                };
            }

            // @resolver/domain: domain directory under resolver
            return match op {
                Op::Lookup(_) => Some(dir_entry(second)),
                Op::List(_) => Some(Self::list_domain()),
                Op::Read(_) => Some(err("not a file")),
            };
        }

        if !is_domain_like(first) {
            return None;
        }

        // domain/<record|_all|_raw>
        Self::record_handler(op, None, first, second)
    }

    /// Three segments: only valid as @resolver/domain/record.
    #[route("/{first}/{second}/{third}")]
    fn three_segments(op: Op, first: &str, second: &str, third: &str) -> Option<ProviderResponse> {
        let resolver = first.strip_prefix('@')?;
        Self::record_handler(op, Some(resolver), second, third)
    }

    // --- Helpers ---

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
            "_all" => match op {
                Op::Lookup(_) => Some(file_entry("_all")),
                Op::Read(id) => Some(Self::read_all(id, resolver, domain)),
                Op::List(_) => Some(err("not a directory")),
            },
            "_raw" => match op {
                Op::Lookup(_) => Some(file_entry("_raw")),
                Op::Read(id) => Some(Self::read_raw(id, resolver, domain)),
                Op::List(_) => Some(err("not a directory")),
            },
            "PTR" => None,
            _ => {
                let rtype = record_name.parse::<RecordType>().ok()?;
                match op {
                    Op::Lookup(_) => Some(file_entry(record_name)),
                    Op::Read(id) => Some(Self::read_record(id, resolver, domain, rtype)),
                    Op::List(_) => Some(err("not a directory")),
                }
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
            Err(e) => return err(&e),
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
            Err(e) => return err(&e),
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
            Err(e) => return err(&e),
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
        let effect = match with_state(|s| doh::reverse_query(&s.resolvers, resolver, ip))
            .and_then(|result| result)
        {
            Ok(effect) => effect,
            Err(e) => return err(&e),
        };
        dispatch(id, Continuation::Single, effect)
    }
}
