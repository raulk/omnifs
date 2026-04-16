use std::net::IpAddr;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    strum::Display,
    strum::EnumString,
    strum::AsRefStr,
    strum::VariantArray,
)]
#[allow(clippy::upper_case_acronyms)]
pub(crate) enum RecordType {
    A,
    AAAA,
    CNAME,
    MX,
    NS,
    TXT,
    SOA,
    SRV,
    CAA,
    PTR,
}

impl RecordType {
    /// PTR excluded: it is only used internally for `_reverse/<ip>`.
    pub fn all() -> &'static [Self] {
        &[
            Self::A,
            Self::AAAA,
            Self::CNAME,
            Self::MX,
            Self::NS,
            Self::TXT,
            Self::SOA,
            Self::SRV,
            Self::CAA,
        ]
    }

    /// Subset queried in parallel for `_all` (skip SRV/CAA to reduce noise).
    pub fn common() -> &'static [Self] {
        &[
            Self::A,
            Self::AAAA,
            Self::CNAME,
            Self::MX,
            Self::NS,
            Self::TXT,
            Self::SOA,
        ]
    }

    pub fn from_wire(num: u16) -> Option<Self> {
        match num {
            1 => Some(Self::A),
            2 => Some(Self::NS),
            5 => Some(Self::CNAME),
            6 => Some(Self::SOA),
            12 => Some(Self::PTR),
            15 => Some(Self::MX),
            16 => Some(Self::TXT),
            28 => Some(Self::AAAA),
            33 => Some(Self::SRV),
            257 => Some(Self::CAA),
            _ => None,
        }
    }
}

/// Parsed path within the DNS provider. Max depth is 3 segments.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum FsPath<'a> {
    Root,
    Resolvers,
    ReverseRoot,
    ReverseIp {
        ip: &'a str,
    },
    DirectReverseIp {
        resolver: Option<&'a str>,
        ip: &'a str,
    },
    Resolver {
        resolver: &'a str,
    },
    Domain {
        resolver: Option<&'a str>,
        domain: &'a str,
    },
    Record {
        resolver: Option<&'a str>,
        domain: &'a str,
        rtype: RecordType,
    },
    All {
        resolver: Option<&'a str>,
        domain: &'a str,
    },
    Raw {
        resolver: Option<&'a str>,
        domain: &'a str,
    },
}

impl<'a> FsPath<'a> {
    pub fn parse(path: &'a str) -> Option<Self> {
        if path.is_empty() {
            return Some(Self::Root);
        }

        // Max depth is 3 (@resolver/domain/record), so splitn(4) detects
        // overflow without allocating a Vec.
        let mut parts = path.splitn(4, '/');
        let seg1 = parts.next()?;
        let seg2 = parts.next();
        let seg3 = parts.next();
        if parts.next().is_some() {
            return None;
        }

        match seg1 {
            "_resolvers" if seg2.is_none() => Some(Self::Resolvers),
            "_reverse" => parse_reverse(seg2, seg3),
            _ => {
                // Normalize: strip optional @resolver prefix, shift segments.
                let (resolver, target, child) = match seg1.strip_prefix('@') {
                    Some(r) => (Some(r), seg2, seg3),
                    None => (None, Some(seg1), seg2),
                };
                parse_target(resolver, target, child)
            }
        }
    }
}

fn parse_reverse<'a>(target: Option<&'a str>, child: Option<&'a str>) -> Option<FsPath<'a>> {
    match (target, child) {
        (None, _) => Some(FsPath::ReverseRoot),
        (Some(ip), None) => Some(FsPath::ReverseIp { ip }),
        _ => None,
    }
}

fn parse_target<'a>(
    resolver: Option<&'a str>,
    target: Option<&'a str>,
    child: Option<&'a str>,
) -> Option<FsPath<'a>> {
    let Some(target) = target else {
        return Some(FsPath::Resolver {
            resolver: resolver?,
        });
    };

    if is_ip_addr(target) {
        return match child {
            None => Some(FsPath::DirectReverseIp {
                resolver,
                ip: target,
            }),
            _ => None,
        };
    }

    if resolver.is_none() && !is_domain_like(target) {
        return None;
    }

    match child {
        None => Some(FsPath::Domain {
            resolver,
            domain: target,
        }),
        Some(record) => parse_record_or_anchor(resolver, target, record),
    }
}

fn parse_record_or_anchor<'a>(
    resolver: Option<&'a str>,
    domain: &'a str,
    name: &'a str,
) -> Option<FsPath<'a>> {
    match name {
        "_all" => Some(FsPath::All { resolver, domain }),
        "_raw" => Some(FsPath::Raw { resolver, domain }),
        "PTR" => None,
        _ => name.parse::<RecordType>().ok().map(|rtype| FsPath::Record {
            resolver,
            domain,
            rtype,
        }),
    }
}

fn is_domain_like(s: &str) -> bool {
    s.contains('.') && !s.contains(char::is_whitespace) && s.len() <= 253
}

fn is_ip_addr(s: &str) -> bool {
    s.parse::<IpAddr>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_root() {
        assert!(matches!(FsPath::parse(""), Some(FsPath::Root)));
    }

    #[test]
    fn parse_domain() {
        let p = FsPath::parse("example.com").unwrap();
        assert!(matches!(
            p,
            FsPath::Domain {
                resolver: None,
                domain: "example.com"
            }
        ));
    }

    #[test]
    fn parse_record() {
        let p = FsPath::parse("example.com/A").unwrap();
        assert!(matches!(
            p,
            FsPath::Record {
                resolver: None,
                domain: "example.com",
                rtype: RecordType::A
            }
        ));
    }

    #[test]
    fn parse_all_anchor() {
        let p = FsPath::parse("example.com/_all").unwrap();
        assert!(matches!(
            p,
            FsPath::All {
                resolver: None,
                domain: "example.com"
            }
        ));
    }

    #[test]
    fn parse_resolver_domain() {
        let p = FsPath::parse("@1.1.1.1/example.com").unwrap();
        assert!(matches!(
            p,
            FsPath::Domain {
                resolver: Some("1.1.1.1"),
                domain: "example.com"
            }
        ));
    }

    #[test]
    fn parse_resolver_record() {
        let p = FsPath::parse("@dns.google/example.com/MX").unwrap();
        assert!(matches!(
            p,
            FsPath::Record {
                resolver: Some("dns.google"),
                domain: "example.com",
                rtype: RecordType::MX
            }
        ));
    }

    #[test]
    fn parse_reverse() {
        let p = FsPath::parse("_reverse/93.184.216.34").unwrap();
        assert!(matches!(
            p,
            FsPath::ReverseIp {
                ip: "93.184.216.34"
            }
        ));
    }

    #[test]
    fn parse_bare_ipv4_as_direct_reverse() {
        let p = FsPath::parse("172.217.171.46").unwrap();
        assert!(matches!(
            p,
            FsPath::DirectReverseIp {
                resolver: None,
                ip: "172.217.171.46"
            }
        ));
    }

    #[test]
    fn parse_bare_ipv6_as_direct_reverse() {
        let p = FsPath::parse("2001:4860:4860::8888").unwrap();
        assert!(matches!(
            p,
            FsPath::DirectReverseIp {
                resolver: None,
                ip: "2001:4860:4860::8888"
            }
        ));
    }

    #[test]
    fn parse_resolver_ipv4_as_direct_reverse() {
        let p = FsPath::parse("@google/172.217.171.46").unwrap();
        assert!(matches!(
            p,
            FsPath::DirectReverseIp {
                resolver: Some("google"),
                ip: "172.217.171.46"
            }
        ));
    }

    #[test]
    fn reject_bare_ip_record_child() {
        assert!(FsPath::parse("172.217.171.46/A").is_none());
    }

    #[test]
    fn reject_reverse_ptr_child() {
        assert!(FsPath::parse("_reverse/93.184.216.34/PTR").is_none());
    }

    #[test]
    fn reject_direct_ptr_record() {
        assert!(FsPath::parse("example.com/PTR").is_none());
    }

    #[test]
    fn parse_resolvers_file() {
        assert!(matches!(
            FsPath::parse("_resolvers"),
            Some(FsPath::Resolvers)
        ));
    }

    #[test]
    fn reject_non_domain() {
        assert!(FsPath::parse("not-a-domain").is_none());
    }

    #[test]
    fn reject_deep_path() {
        assert!(FsPath::parse("@r/example.com/A/extra").is_none());
    }

    #[test]
    fn all_covers_all_non_ptr_variants() {
        use strum::VariantArray;
        let expected: Vec<_> = RecordType::VARIANTS
            .iter()
            .filter(|v| **v != RecordType::PTR)
            .collect();
        let actual: Vec<_> = RecordType::all().iter().collect();
        assert_eq!(actual, expected, "all() must match VARIANTS minus PTR");
    }
}
