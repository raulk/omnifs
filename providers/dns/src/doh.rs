use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::collections::BTreeMap;
use std::fmt::Write;

use hickory_proto::op::{Message, ResponseCode};
#[cfg(test)]
use hickory_proto::rr::{Name, RData, Record, rdata::A};

use crate::types::RecordType;
use omnifs_sdk::prelude::*;

const CLOUDFLARE_DOH: &str = "https://cloudflare-dns.com/dns-query";
const GOOGLE_DOH: &str = "https://dns.google/dns-query";
const BUILTIN_DEFAULTS_JSON: &str = r#"{
  "default_resolver": "cloudflare",
  "resolvers": {
    "cloudflare": {
      "url": "https://cloudflare-dns.com/dns-query",
      "aliases": ["1.1.1.1", "1.0.0.1"]
    },
    "google": {
      "url": "https://dns.google/dns-query",
      "aliases": ["8.8.8.8", "8.8.4.4", "dns.google"]
    }
  }
}"#;

#[derive(serde::Deserialize)]
#[serde(default)]
struct RawConfig {
    default_resolver: String,
    #[serde(default)]
    resolvers: BTreeMap<String, RawResolver>,
}

impl Default for RawConfig {
    fn default() -> Self {
        Self {
            default_resolver: "cloudflare".to_string(),
            resolvers: BTreeMap::new(),
        }
    }
}

#[derive(serde::Deserialize)]
struct RawResolver {
    url: String,
    #[serde(default)]
    aliases: Vec<String>,
}

fn parse_raw_resolvers(bytes: &[u8]) -> RawConfig {
    omnifs_sdk::serde_json::from_slice(bytes).unwrap_or_default()
}

fn build_resolver_entries(
    raw_resolvers: BTreeMap<String, RawResolver>,
) -> Vec<ResolverEntry> {
    raw_resolvers
        .into_iter()
        .filter_map(|(name, raw)| {
            Some(ResolverEntry {
                name,
                url: Endpoint::new(raw.url).ok()?,
                aliases: raw.aliases,
            })
        })
        .collect()
}

#[derive(Debug)]
enum DohError {
    Parse(String),
    DnsResponse(ResponseCode),
}

impl DohError {
    fn into_provider_error(self) -> ProviderError {
        match self {
            Self::Parse(message) => {
                ProviderError::invalid_input(format!("invalid DoH DNS message: {message}"))
            }
            Self::DnsResponse(code) => {
                let message = format!("DNS response code: {code}");
                match code {
                    ResponseCode::FormErr => ProviderError::invalid_input(message),
                    ResponseCode::ServFail => ProviderError::network(message, true),
                    ResponseCode::NXDomain => ProviderError::not_found(message),
                    ResponseCode::Refused => ProviderError::denied(message),
                    _ => ProviderError::internal(message),
                }
            }
        }
    }
}

/// Validated `DoH` endpoint URL (always HTTPS).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Endpoint(String);

impl Endpoint {
    fn new(url: impl Into<String>) -> Result<Self, String> {
        let url = url.into();
        if !url.starts_with("https://") {
            return Err(format!("DoH endpoint must use HTTPS: {url}"));
        }
        Ok(Self(url))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<&str> for Endpoint {
    fn eq(&self, other: &&str) -> bool {
        self.0.as_str() == *other
    }
}

/// Resolver aliases and their `DoH` endpoints, parsed from provider config.
///
/// Example JSON (as received by the provider, `config` object only):
/// ```json
/// {
///   "default_resolver": "cloudflare",
///   "resolvers": {
///     "cloudflare": {
///       "url": "https://cloudflare-dns.com/dns-query",
///       "aliases": ["1.1.1.1", "1.0.0.1"]
///     }
///   }
/// }
/// ```
#[derive(Debug, Clone)]
pub(crate) struct ResolverConfig {
    pub default_name: String,
    pub resolvers: Vec<ResolverEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolverEntry {
    pub name: String,
    pub url: Endpoint,
    pub aliases: Vec<String>,
}

impl ResolverConfig {
    /// Build from already-deserialized config maps (called from `init`).
    pub fn from_config<I>(default_resolver: String, raw_resolvers: I) -> Self
    where
        I: IntoIterator<Item = (String, crate::ConfigResolver)>,
    {
        let resolvers: Vec<_> = raw_resolvers
            .into_iter()
            .filter_map(|(name, r)| {
                Some(ResolverEntry {
                    name,
                    url: Endpoint::new(r.url).ok()?,
                    aliases: r.aliases,
                })
            })
            .collect();

        Self {
            default_name: default_resolver,
            resolvers: if resolvers.is_empty() {
                Self::builtin_defaults()
            } else {
                resolvers
            },
        }
    }

    /// Build from raw JSON bytes (used by tests only).
    #[cfg(test)]
    pub fn from_json(config_bytes: &[u8]) -> Self {
        let raw = parse_raw_resolvers(config_bytes);
        let resolvers = build_resolver_entries(raw.resolvers);

        Self {
            default_name: raw.default_resolver,
            resolvers: if resolvers.is_empty() {
                Self::builtin_defaults()
            } else {
                resolvers
            },
        }
    }

    fn builtin_defaults() -> Vec<ResolverEntry> {
        let raw = parse_raw_resolvers(BUILTIN_DEFAULTS_JSON.as_bytes());
        build_resolver_entries(raw.resolvers)
    }

    pub fn resolve_endpoint(&self, specifier: Option<&str>) -> Endpoint {
        let Some(spec) = specifier else {
            return self.default_endpoint();
        };

        Endpoint::new(spec)
            .ok()
            .or_else(|| self.lookup(spec))
            .unwrap_or_else(|| self.default_endpoint())
    }

    fn lookup(&self, spec: &str) -> Option<Endpoint> {
        self.resolvers
            .iter()
            .find(|e| e.name == spec || e.aliases.iter().any(|a| a == spec))
            .map(|e| e.url.clone())
    }

    fn default_endpoint(&self) -> Endpoint {
        self.lookup(&self.default_name)
            .or_else(|| self.resolvers.first().map(|e| e.url.clone()))
            .unwrap_or_else(|| Endpoint::new(CLOUDFLARE_DOH).expect("hardcoded URL"))
    }

    /// Format `_resolvers` file content from configured resolvers.
    pub fn format_resolvers_file(&self) -> String {
        self.resolvers
            .iter()
            .map(|e| format!("{}\t{}\t{}", e.name, e.aliases.join(","), e.url))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }

    /// Resolver names prefixed with `@` for directory listing.
    pub fn resolver_dir_names(&self) -> Vec<String> {
        self.resolvers
            .iter()
            .map(|e| format!("@{}", e.name))
            .collect()
    }
}

pub(crate) fn query(
    config: &ResolverConfig,
    resolver: Option<&str>,
    domain: &str,
    rtype: RecordType,
) -> SingleEffect {
    let endpoint = config.resolve_endpoint(resolver);
    let ep = endpoint.as_str();
    let sep = if ep.contains('?') { '&' } else { '?' };
    let url = format!("{ep}{sep}name={domain}&type={rtype}");

    SingleEffect::Fetch(HttpRequest {
        method: "GET".to_string(),
        url,
        headers: vec![Header {
            name: "Accept".to_string(),
            value: "application/dns-message".to_string(),
        }],
        body: None,
    })
}

pub(crate) fn parse_response(body: &[u8]) -> Result<(Vec<crate::DnsRecord>, u64), ProviderError> {
    parse_doh_response(body).map_err(DohError::into_provider_error)
}

fn parse_doh_response(body: &[u8]) -> Result<(Vec<crate::DnsRecord>, u64), DohError> {
    let response = Message::from_vec(body).map_err(|e| DohError::Parse(e.to_string()))?;

    if response.response_code() != ResponseCode::NoError {
        return Err(DohError::DnsResponse(response.response_code()));
    }

    let mut min_ttl = u64::MAX;
    let mut records = Vec::new();

    for answer in response.answers() {
        let maybe_type = RecordType::from_wire(u16::from(answer.record_type()));
        if let Some(rtype) = maybe_type {
            min_ttl = min_ttl.min(u64::from(answer.ttl()));
            records.push(crate::DnsRecord {
                rtype,
                value: answer.data().to_string(),
            });
        }
    }

    Ok((records, if min_ttl == u64::MAX { 300 } else { min_ttl }))
}

pub(crate) fn reverse_query(
    config: &ResolverConfig,
    resolver: Option<&str>,
    ip: &str,
) -> Result<SingleEffect, ProviderError> {
    let addr = ip
        .parse::<IpAddr>()
        .map_err(|_| ProviderError::invalid_input(format!("invalid IP address: {ip}")))?;
    let ptr_domain = match addr {
        IpAddr::V4(addr) => ip_to_in_addr_arpa(addr),
        IpAddr::V6(addr) => ip_to_ip6_arpa(addr),
    };
    Ok(query(config, resolver, &ptr_domain, RecordType::PTR))
}

fn ip_to_in_addr_arpa(ip: Ipv4Addr) -> String {
    let octets = ip.octets();
    format!(
        "{}.{}.{}.{}.in-addr.arpa",
        octets[3], octets[2], octets[1], octets[0]
    )
}

fn ip_to_ip6_arpa(ip: Ipv6Addr) -> String {
    // 32 nibbles + 31 dots + ".ip6.arpa" (9) = 72
    let mut out = String::with_capacity(72);
    for (i, nibble) in ip
        .octets()
        .iter()
        .rev()
        .flat_map(|byte| [byte & 0x0f, byte >> 4])
        .enumerate()
    {
        if i > 0 {
            out.push('.');
        }
        let _ = write!(out, "{nibble:x}");
    }
    out.push_str(".ip6.arpa");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ResolverConfig {
        ResolverConfig::from_json(b"{}")
    }

    #[test]
    fn default_resolves_to_cloudflare() {
        let cfg = default_config();
        assert_eq!(cfg.resolve_endpoint(None), CLOUDFLARE_DOH);
        assert_eq!(cfg.resolve_endpoint(Some("cloudflare")), CLOUDFLARE_DOH);
        assert_eq!(cfg.resolve_endpoint(Some("1.1.1.1")), CLOUDFLARE_DOH);
    }

    #[test]
    fn alias_resolves_google() {
        let cfg = default_config();
        assert_eq!(cfg.resolve_endpoint(Some("google")), GOOGLE_DOH);
        assert_eq!(cfg.resolve_endpoint(Some("8.8.8.8")), GOOGLE_DOH);
        assert_eq!(cfg.resolve_endpoint(Some("dns.google")), GOOGLE_DOH);
    }

    #[test]
    fn custom_resolver_from_config() {
        let json = br#"{
            "default_resolver": "quad9",
            "resolvers": {
                "quad9": {
                    "url": "https://dns.quad9.net:5053/dns-query",
                    "aliases": ["9.9.9.9"]
                }
            }
        }"#;
        let cfg = ResolverConfig::from_json(json);
        assert_eq!(
            cfg.resolve_endpoint(None),
            "https://dns.quad9.net:5053/dns-query"
        );
        assert_eq!(
            cfg.resolve_endpoint(Some("9.9.9.9")),
            "https://dns.quad9.net:5053/dns-query"
        );
    }

    #[test]
    fn https_url_passthrough() {
        let cfg = default_config();
        assert_eq!(
            cfg.resolve_endpoint(Some("https://custom.dns/query")),
            "https://custom.dns/query"
        );
    }

    #[test]
    fn unknown_falls_back_to_default() {
        let cfg = default_config();
        assert_eq!(cfg.resolve_endpoint(Some("unknown")), CLOUDFLARE_DOH);
    }

    #[test]
    fn in_addr_arpa() {
        assert_eq!(
            ip_to_in_addr_arpa(Ipv4Addr::new(93, 184, 216, 34)),
            "34.216.184.93.in-addr.arpa"
        );
    }

    #[test]
    fn ip6_arpa() {
        assert_eq!(
            ip_to_ip6_arpa(Ipv6Addr::LOCALHOST),
            "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.ip6.arpa"
        );
    }

    #[test]
    fn invalid_reverse_ip_is_rejected() {
        let cfg = default_config();
        assert!(matches!(
            reverse_query(&cfg, None, "1:2:3:4:5:6:7:8:9"),
            Err(err) if format!("{err}").contains("invalid IP address")
        ));
        assert!(matches!(
            reverse_query(&cfg, None, "999.184.216.34"),
            Err(err) if format!("{err}").contains("invalid IP address")
        ));
    }

    #[test]
    fn parse_doh_response() {
        let response = make_dns_message_response(
            ResponseCode::NoError,
            [
                ("example.com", 300, "93.184.216.34"),
                ("example.com", 200, "93.184.216.35"),
            ],
        );
        let (records, ttl) = parse_response(&response).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].value, "93.184.216.34");
        assert_eq!(records[1].value, "93.184.216.35");
        assert_eq!(ttl, 200);
    }

    #[test]
    fn parse_nxdomain() {
        let response = make_dns_message_response(ResponseCode::NXDomain, []);
        let err = parse_response(&response).unwrap_err();
        assert!(err.to_string().contains("NXDOMAIN"));
    }

    fn make_dns_message_response(
        response_code: ResponseCode,
        answers: impl IntoIterator<Item = (&'static str, u32, &'static str)>,
    ) -> Vec<u8> {
        let mut response = Message::new();
        response.set_response_code(response_code);
        let records: Vec<_> = answers
            .into_iter()
            .map(|(domain, ttl, address)| make_a_record(domain, ttl, address))
            .collect();
        response.add_answers(records);
        response.to_vec().unwrap()
    }

    fn make_a_record(domain: &str, ttl: u32, address: &str) -> Record {
        Record::from_rdata(
            Name::from_ascii(domain).unwrap(),
            ttl,
            RData::A(A(address.parse::<std::net::Ipv4Addr>().unwrap())),
        )
    }

    #[test]
    fn resolvers_file_format() {
        let cfg = default_config();
        let content = cfg.format_resolvers_file();
        assert!(content.contains("cloudflare"));
        assert!(content.contains("google"));
        assert!(content.contains(CLOUDFLARE_DOH));
    }
}
