#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    pub fn as_str(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::AAAA => "AAAA",
            Self::CNAME => "CNAME",
            Self::MX => "MX",
            Self::NS => "NS",
            Self::TXT => "TXT",
            Self::SOA => "SOA",
            Self::SRV => "SRV",
            Self::CAA => "CAA",
            Self::PTR => "PTR",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "A" => Some(Self::A),
            "AAAA" => Some(Self::AAAA),
            "CNAME" => Some(Self::CNAME),
            "MX" => Some(Self::MX),
            "NS" => Some(Self::NS),
            "TXT" => Some(Self::TXT),
            "SOA" => Some(Self::SOA),
            "SRV" => Some(Self::SRV),
            "CAA" => Some(Self::CAA),
            "PTR" => Some(Self::PTR),
            _ => None,
        }
    }

    /// PTR excluded: only reachable via `_reverse/<ip>`, not as a
    /// direct child of a domain directory.
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

        // Reject paths deeper than 3 segments.
        if parts.next().is_some() {
            return None;
        }

        match (seg1, seg2, seg3) {
            ("_resolvers", None, None) => Some(Self::Resolvers),
            ("_reverse", None, None) => Some(Self::ReverseRoot),
            ("_reverse", Some(ip), None) => Some(Self::ReverseIp { ip }),

            (resolver, None, None) if resolver.starts_with('@') => Some(Self::Resolver {
                resolver: &resolver[1..],
            }),
            (resolver, Some(domain), None) if resolver.starts_with('@') => Some(Self::Domain {
                resolver: Some(&resolver[1..]),
                domain,
            }),
            (resolver, Some(domain), Some(record)) if resolver.starts_with('@') => {
                parse_record_or_anchor(Some(&resolver[1..]), domain, record)
            }

            (domain, None, None) if is_domain_like(domain) => Some(Self::Domain {
                resolver: None,
                domain,
            }),
            (domain, Some(record), None) if is_domain_like(domain) => {
                parse_record_or_anchor(None, domain, record)
            }

            _ => None,
        }
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
        _ => RecordType::from_str(name).map(|rtype| FsPath::Record {
            resolver,
            domain,
            rtype,
        }),
    }
}

fn is_domain_like(s: &str) -> bool {
    s.contains('.') && !s.contains(char::is_whitespace) && s.len() <= 253
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
        assert!(matches!(p, FsPath::ReverseIp { ip: "93.184.216.34" }));
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
        // Guard against adding a variant to RecordType without updating all().
        for rt in RecordType::all() {
            assert_eq!(RecordType::from_str(rt.as_str()), Some(*rt));
        }
        // PTR is intentionally excluded from all() but must still parse.
        assert_eq!(RecordType::from_str("PTR"), Some(RecordType::PTR));
    }
}
