/// Record types supported by the provider.
///
/// Each variant maps to a DNS RR type and corresponds to a filename
/// inside a domain directory (e.g. `/dns/example.com/A`).
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

    /// All record types shown in a domain directory listing.
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

    /// Common types queried for `_all` (skip SRV/CAA to reduce noise).
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

/// Parsed filesystem path within the DNS provider.
///
/// Hierarchy:
///   ""                                  -> provider root
///   "@<resolver>"                       -> resolver directory
///   "@<resolver>/<domain>"              -> domain under resolver
///   "@<resolver>/<domain>/<record>"     -> record file under resolver
///   "<domain>"                          -> domain directory (default resolver)
///   "<domain>/<record>"                 -> record file
///   "_reverse"                          -> reverse lookup namespace
///   "_reverse/<ip>"                     -> PTR result
///   "_resolvers"                        -> list of known resolvers
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

        let segments: Vec<&str> = path.split('/').collect();

        match segments.as_slice() {
            ["_resolvers"] => Some(Self::Resolvers),
            ["_reverse"] => Some(Self::ReverseRoot),
            ["_reverse", ip] => Some(Self::ReverseIp { ip }),

            [resolver] if resolver.starts_with('@') => Some(Self::Resolver {
                resolver: &resolver[1..],
            }),
            [resolver, domain] if resolver.starts_with('@') => Some(Self::Domain {
                resolver: Some(&resolver[1..]),
                domain,
            }),
            [resolver, domain, record] if resolver.starts_with('@') => {
                parse_record_or_anchor(Some(&resolver[1..]), domain, record)
            }

            [domain] if is_domain_like(domain) => Some(Self::Domain {
                resolver: None,
                domain,
            }),
            [domain, record] if is_domain_like(domain) => {
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
}
