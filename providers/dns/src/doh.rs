use std::fmt::Write;

use crate::omnifs::provider::types::*;
use crate::path::RecordType;

const CLOUDFLARE_DOH: &str = "https://cloudflare-dns.com/dns-query";
const GOOGLE_DOH: &str = "https://dns.google/resolve";

pub(crate) fn resolve_endpoint(resolver: Option<&str>) -> &str {
    match resolver {
        None | Some("cloudflare" | "1.1.1.1" | "1.0.0.1") => CLOUDFLARE_DOH,
        Some("google" | "8.8.8.8" | "8.8.4.4" | "dns.google") => GOOGLE_DOH,
        Some(other) if other.starts_with("https") => other,
        Some(_) => CLOUDFLARE_DOH,
    }
}

pub(crate) fn query(resolver: Option<&str>, domain: &str, rtype: RecordType) -> SingleEffect {
    let endpoint = resolve_endpoint(resolver);
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

pub(crate) fn query_batch(
    resolver: Option<&str>,
    domain: &str,
    types: &[RecordType],
) -> Vec<SingleEffect> {
    types
        .iter()
        .map(|&rt| query(resolver, domain, rt))
        .collect()
}

pub(crate) fn parse_response(body: &[u8]) -> Result<(Vec<crate::DnsRecord>, u64), String> {
    let json: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| format!("invalid DoH JSON: {e}"))?;

    let status = json
        .get("Status")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if status != 0 {
        let status_name = match status {
            1 => "FORMERR",
            2 => "SERVFAIL",
            3 => "NXDOMAIN",
            5 => "REFUSED",
            _ => "ERROR",
        };
        return Err(format!("DNS {status_name} (status {status})"));
    }

    let empty = Vec::new();
    let answers = json
        .get("Answer")
        .and_then(|v| v.as_array())
        .unwrap_or(&empty);

    let mut min_ttl = u64::MAX;
    let mut records = Vec::new();

    for answer in answers {
        #[allow(clippy::cast_possible_truncation)]
        let type_num = answer
            .get("type")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u16;
        let data = answer
            .get("data")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let ttl = answer
            .get("TTL")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(300);
        min_ttl = min_ttl.min(ttl);

        if let Some(rtype) = type_num_to_record(type_num) {
            records.push(crate::DnsRecord { rtype, value: data });
        }
    }

    if min_ttl == u64::MAX {
        min_ttl = 300;
    }

    Ok((records, min_ttl))
}

pub(crate) fn reverse_query(resolver: Option<&str>, ip: &str) -> SingleEffect {
    let ptr_domain = if ip.contains(':') {
        ip_to_ip6_arpa(ip)
    } else {
        ip_to_in_addr_arpa(ip)
    };
    query(resolver, &ptr_domain, RecordType::PTR)
}

fn ip_to_in_addr_arpa(ip: &str) -> String {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() == 4 {
        format!(
            "{}.{}.{}.{}.in-addr.arpa",
            parts[3], parts[2], parts[1], parts[0]
        )
    } else {
        format!("{ip}.in-addr.arpa")
    }
}

fn ip_to_ip6_arpa(ip: &str) -> String {
    let expanded = expand_ipv6(ip);
    // 32 nibbles + 31 dots + ".ip6.arpa" (9) = 72
    let mut out = String::with_capacity(72);
    let mut first = true;
    for c in expanded.chars().filter(|c| *c != ':').rev() {
        if !first {
            out.push('.');
        }
        out.push(c);
        first = false;
    }
    out.push_str(".ip6.arpa");
    out
}

fn expand_ipv6(ip: &str) -> String {
    // Pre-allocate: 8 groups * 4 chars + 7 colons = 39
    let mut out = String::with_capacity(39);
    let groups: Vec<&str> = ip.split("::").collect();

    let (left_parts, right_parts): (Vec<&str>, Vec<&str>) =
        if let [left, right] = groups.as_slice() {
            let l: Vec<&str> = if left.is_empty() {
                vec![]
            } else {
                left.split(':').collect()
            };
            let r: Vec<&str> = if right.is_empty() {
                vec![]
            } else {
                right.split(':').collect()
            };
            (l, r)
        } else {
            (ip.split(':').collect(), vec![])
        };

    let missing = 8 - left_parts.len() - right_parts.len();

    for (i, p) in left_parts
        .iter()
        .chain(std::iter::repeat_n(&"0", missing))
        .chain(right_parts.iter())
        .enumerate()
    {
        if i > 0 {
            out.push(':');
        }
        let _ = write!(out, "{p:0>4}");
    }
    out
}

fn type_num_to_record(num: u16) -> Option<RecordType> {
    match num {
        1 => Some(RecordType::A),
        28 => Some(RecordType::AAAA),
        5 => Some(RecordType::CNAME),
        15 => Some(RecordType::MX),
        2 => Some(RecordType::NS),
        16 => Some(RecordType::TXT),
        6 => Some(RecordType::SOA),
        33 => Some(RecordType::SRV),
        257 => Some(RecordType::CAA),
        12 => Some(RecordType::PTR),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_default_is_cloudflare() {
        assert_eq!(resolve_endpoint(None), CLOUDFLARE_DOH);
        assert_eq!(resolve_endpoint(Some("cloudflare")), CLOUDFLARE_DOH);
        assert_eq!(resolve_endpoint(Some("1.1.1.1")), CLOUDFLARE_DOH);
    }

    #[test]
    fn resolve_google() {
        assert_eq!(resolve_endpoint(Some("google")), GOOGLE_DOH);
        assert_eq!(resolve_endpoint(Some("8.8.8.8")), GOOGLE_DOH);
        assert_eq!(resolve_endpoint(Some("dns.google")), GOOGLE_DOH);
    }

    #[test]
    fn in_addr_arpa() {
        assert_eq!(
            ip_to_in_addr_arpa("93.184.216.34"),
            "34.216.184.93.in-addr.arpa"
        );
    }

    #[test]
    fn ipv6_expansion() {
        assert_eq!(
            expand_ipv6("::1"),
            "0000:0000:0000:0000:0000:0000:0000:0001"
        );
        assert_eq!(
            expand_ipv6("2606:2800:220:1:248:1893:25c8:1946"),
            "2606:2800:0220:0001:0248:1893:25c8:1946"
        );
    }

    #[test]
    fn ip6_arpa() {
        assert_eq!(
            ip_to_ip6_arpa("::1"),
            "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.ip6.arpa"
        );
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
}
