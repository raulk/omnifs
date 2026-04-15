use std::fmt::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::omnifs::provider::types::*;
use crate::path::RecordType;

const CLOUDFLARE_DOH: &str = "https://cloudflare-dns.com/dns-query";
const GOOGLE_DOH: &str = "https://dns.google/resolve";

/// Resolver aliases and their `DoH` endpoints, parsed from provider config.
///
/// Example TOML:
/// ```toml
/// [config]
/// default_resolver = "cloudflare"
///
/// [config.resolvers]
/// cloudflare = { url = "https://cloudflare-dns.com/dns-query", aliases = ["1.1.1.1", "1.0.0.1"] }
/// google = { url = "https://dns.google/resolve", aliases = ["8.8.8.8", "8.8.4.4", "dns.google"] }
/// quad9 = { url = "https://dns.quad9.net:5053/dns-query", aliases = ["9.9.9.9"] }
/// ```
#[derive(Debug, Clone)]
pub(crate) struct ResolverConfig {
    pub default_name: String,
    pub resolvers: Vec<ResolverEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolverEntry {
    pub name: String,
    pub url: String,
    pub aliases: Vec<String>,
}

impl ResolverConfig {
    pub fn from_toml(config_bytes: &[u8]) -> Self {
        let toml_str = std::str::from_utf8(config_bytes).unwrap_or("");
        let mut parsed = parse_resolver_config(toml_str);

        if parsed.resolvers.is_empty() {
            parsed.resolvers = Self::builtin_defaults();
        }

        Self {
            default_name: parsed.default_name,
            resolvers: parsed.resolvers,
        }
    }

    fn builtin_defaults() -> Vec<ResolverEntry> {
        vec![
            ResolverEntry {
                name: "cloudflare".to_string(),
                url: CLOUDFLARE_DOH.to_string(),
                aliases: vec!["1.1.1.1".to_string(), "1.0.0.1".to_string()],
            },
            ResolverEntry {
                name: "google".to_string(),
                url: GOOGLE_DOH.to_string(),
                aliases: vec![
                    "8.8.8.8".to_string(),
                    "8.8.4.4".to_string(),
                    "dns.google".to_string(),
                ],
            },
        ]
    }

    pub fn resolve_endpoint<'a>(&'a self, specifier: Option<&'a str>) -> &'a str {
        let Some(spec) = specifier else {
            return self.default_endpoint();
        };

        // Raw URL passthrough.
        if spec.starts_with("https") {
            return spec;
        }

        for entry in &self.resolvers {
            if entry.name == spec || entry.aliases.iter().any(|a| a == spec) {
                return &entry.url;
            }
        }

        self.default_endpoint()
    }

    fn default_endpoint(&self) -> &str {
        for entry in &self.resolvers {
            if entry.name == self.default_name {
                return &entry.url;
            }
        }
        // Fallback if default_name doesn't match any entry.
        self.resolvers
            .first()
            .map_or(CLOUDFLARE_DOH, |e| e.url.as_str())
    }

    /// Format `_resolvers` file content from configured resolvers.
    pub fn format_resolvers_file(&self) -> String {
        let mut out = String::new();
        for entry in &self.resolvers {
            let aliases = if entry.aliases.is_empty() {
                String::new()
            } else {
                entry.aliases.join(",")
            };
            let _ = writeln!(out, "{}\t{}\t{}", entry.name, aliases, entry.url);
        }
        out
    }

    /// Resolver names prefixed with `@` for directory listing.
    pub fn resolver_dir_names(&self) -> Vec<String> {
        self.resolvers
            .iter()
            .map(|e| format!("@{}", e.name))
            .collect()
    }
}

struct ParsedResolverConfig {
    default_name: String,
    resolvers: Vec<ResolverEntry>,
}

#[derive(Default)]
struct ResolverBuilder {
    name: String,
    url: Option<String>,
    aliases: Vec<String>,
}

impl ResolverBuilder {
    fn into_entry(self) -> Option<ResolverEntry> {
        Some(ResolverEntry {
            name: self.name,
            url: self.url?,
            aliases: self.aliases,
        })
    }
}

#[derive(Default)]
enum ConfigSection {
    #[default]
    Root,
    Resolvers,
    ResolverTable,
    Other,
}

fn parse_resolver_config(input: &str) -> ParsedResolverConfig {
    let mut parsed = ParsedResolverConfig {
        default_name: "cloudflare".to_string(),
        resolvers: Vec::new(),
    };
    let mut section = ConfigSection::Root;
    let mut current_resolver: Option<ResolverBuilder> = None;

    for raw_line in input.lines() {
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }

        if let Some(section_name) = parse_section(line) {
            flush_resolver(&mut parsed.resolvers, &mut current_resolver);
            if section_name == "resolvers" {
                section = ConfigSection::Resolvers;
            } else if let Some(name) = section_name.strip_prefix("resolvers.") {
                current_resolver = Some(ResolverBuilder {
                    name: parse_key(name),
                    ..ResolverBuilder::default()
                });
                section = ConfigSection::ResolverTable;
            } else {
                section = ConfigSection::Other;
            }
            continue;
        }

        let Some((key, value)) = split_key_value(line) else {
            continue;
        };

        match section {
            ConfigSection::Root => {
                if key == "default_resolver"
                    && let Some(default_name) = parse_string(value)
                {
                    parsed.default_name = default_name;
                }
            }
            ConfigSection::Resolvers => {
                if let Some(entry) = parse_inline_resolver(key, value) {
                    parsed.resolvers.push(entry);
                }
            }
            ConfigSection::ResolverTable => {
                if let Some(builder) = current_resolver.as_mut() {
                    match key {
                        "url" => builder.url = parse_string(value),
                        "aliases" => builder.aliases = parse_string_array(value),
                        _ => {}
                    }
                }
            }
            ConfigSection::Other => {}
        }
    }

    flush_resolver(&mut parsed.resolvers, &mut current_resolver);
    parsed
}

fn flush_resolver(entries: &mut Vec<ResolverEntry>, builder: &mut Option<ResolverBuilder>) {
    if let Some(entry) = builder.take().and_then(ResolverBuilder::into_entry) {
        entries.push(entry);
    }
}

fn parse_section(line: &str) -> Option<&str> {
    line.strip_prefix('[')?.strip_suffix(']').map(str::trim)
}

fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in line.char_indices() {
        match ch {
            '\\' if in_string => escaped = !escaped,
            '"' if !escaped => in_string = !in_string,
            '=' if !in_string => {
                return Some((line[..index].trim(), line[index + 1..].trim()));
            }
            _ => escaped = false,
        }
    }

    None
}

fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in line.char_indices() {
        match ch {
            '\\' if in_string => escaped = !escaped,
            '"' if !escaped => in_string = !in_string,
            '#' if !in_string => return &line[..index],
            _ => escaped = false,
        }
    }

    line
}

fn parse_key(key: &str) -> String {
    parse_string(key).unwrap_or_else(|| key.trim().to_string())
}

fn parse_string(value: &str) -> Option<String> {
    let value = value.trim();
    let rest = value.strip_prefix('"')?;
    let mut parsed = String::new();
    let mut escaped = false;

    for ch in rest.chars() {
        if escaped {
            match ch {
                '"' => parsed.push('"'),
                '\\' => parsed.push('\\'),
                'n' => parsed.push('\n'),
                'r' => parsed.push('\r'),
                't' => parsed.push('\t'),
                other => parsed.push(other),
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(parsed);
        } else {
            parsed.push(ch);
        }
    }

    None
}

fn parse_string_array(value: &str) -> Vec<String> {
    let Some(inner) = value
        .trim()
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
    else {
        return Vec::new();
    };

    split_comma_separated(inner)
        .into_iter()
        .filter_map(parse_string)
        .collect()
}

fn parse_inline_resolver(key: &str, value: &str) -> Option<ResolverEntry> {
    let inner = value
        .trim()
        .strip_prefix('{')
        .and_then(|v| v.strip_suffix('}'))?;
    let mut builder = ResolverBuilder {
        name: parse_key(key),
        ..ResolverBuilder::default()
    };

    for field in split_comma_separated(inner) {
        let Some((field_name, field_value)) = split_key_value(field) else {
            continue;
        };
        match field_name {
            "url" => builder.url = parse_string(field_value),
            "aliases" => builder.aliases = parse_string_array(field_value),
            _ => {}
        }
    }

    builder.into_entry()
}

fn split_comma_separated(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut depth = 0_u32;

    for (index, ch) in input.char_indices() {
        match ch {
            '\\' if in_string => escaped = !escaped,
            '"' if !escaped => in_string = !in_string,
            '[' | '{' if !in_string => {
                depth = depth.saturating_add(1);
                escaped = false;
            }
            ']' | '}' if !in_string => {
                depth = depth.saturating_sub(1);
                escaped = false;
            }
            ',' if !in_string && depth == 0 => {
                parts.push(input[start..index].trim());
                start = index + 1;
                escaped = false;
            }
            _ => escaped = false,
        }
    }

    parts.push(input[start..].trim());
    parts
}

pub(crate) fn query(
    config: &ResolverConfig,
    resolver: Option<&str>,
    domain: &str,
    rtype: RecordType,
) -> SingleEffect {
    let endpoint = config.resolve_endpoint(resolver);
    let sep = if endpoint.contains('?') { '&' } else { '?' };
    let url = format!("{endpoint}{sep}name={domain}&type={}", rtype.as_str());

    SingleEffect::Fetch(HttpRequest {
        method: "GET".to_string(),
        url,
        headers: vec![Header {
            name: "Accept".to_string(),
            value: "application/dns-json".to_string(),
        }],
        body: None,
    })
}

/// `DoH` JSON response (RFC 8484 / Cloudflare/Google convention).
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DohResponse {
    status: u64,
    #[serde(default)]
    answer: Vec<DohAnswer>,
}

#[derive(serde::Deserialize)]
struct DohAnswer {
    #[serde(rename = "type", default)]
    rtype: u16,
    #[serde(rename = "TTL", default = "default_ttl")]
    ttl: u64,
    #[serde(default)]
    data: String,
}

fn default_ttl() -> u64 {
    300
}

pub(crate) fn parse_response(body: &[u8]) -> Result<(Vec<crate::DnsRecord>, u64), String> {
    let resp: DohResponse =
        serde_json::from_slice(body).map_err(|e| format!("invalid DoH JSON: {e}"))?;

    if resp.status != 0 {
        let name = match resp.status {
            1 => "FORMERR",
            2 => "SERVFAIL",
            3 => "NXDOMAIN",
            5 => "REFUSED",
            _ => "ERROR",
        };
        return Err(format!("DNS {name} (status {})", resp.status));
    }

    let mut min_ttl = u64::MAX;
    let mut records = Vec::new();

    for a in &resp.answer {
        min_ttl = min_ttl.min(a.ttl);
        if let Some(rtype) = RecordType::from_wire(a.rtype) {
            records.push(crate::DnsRecord {
                rtype,
                value: a.data.clone(),
            });
        }
    }

    Ok((records, if min_ttl == u64::MAX { 300 } else { min_ttl }))
}

pub(crate) fn reverse_query(
    config: &ResolverConfig,
    resolver: Option<&str>,
    ip: &str,
) -> Result<SingleEffect, String> {
    let addr = ip
        .parse::<IpAddr>()
        .map_err(|_| format!("invalid IP address: {ip}"))?;
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
        ResolverConfig::from_toml(b"")
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
        let toml = br#"
            default_resolver = "quad9"

            [resolvers]
            quad9 = { url = "https://dns.quad9.net:5053/dns-query", aliases = ["9.9.9.9"] }
        "#;
        let cfg = ResolverConfig::from_toml(toml);
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
            Err(err) if err.contains("invalid IP address")
        ));
        assert!(matches!(
            reverse_query(&cfg, None, "999.184.216.34"),
            Err(err) if err.contains("invalid IP address")
        ));
    }

    #[test]
    fn parse_doh_response() {
        let json = br#"{
            "Status": 0,
            "Answer": [
                {"name": "example.com.", "type": 1, "TTL": 300, "data": "93.184.216.34"},
                {"name": "example.com.", "type": 1, "TTL": 200, "data": "93.184.216.35"}
            ]
        }"#;
        let (records, ttl) = parse_response(json).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].value, "93.184.216.34");
        assert_eq!(records[1].value, "93.184.216.35");
        assert_eq!(ttl, 200);
    }

    #[test]
    fn parse_nxdomain() {
        let err = parse_response(br#"{"Status": 3}"#).unwrap_err();
        assert!(err.contains("NXDOMAIN"));
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
