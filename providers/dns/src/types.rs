use std::net::IpAddr;
use std::str::FromStr;

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
    num_enum::TryFromPrimitive,
)]
#[repr(u16)]
#[allow(clippy::upper_case_acronyms)]
pub(crate) enum RecordType {
    A = 1,
    AAAA = 28,
    CNAME = 5,
    MX = 15,
    NS = 2,
    TXT = 16,
    SOA = 6,
    SRV = 33,
    CAA = 257,
    PTR = 12,
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
        Self::try_from(num).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct DomainName(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ResolverName(String);

impl FromStr for DomainName {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        (s.parse::<IpAddr>().is_err()
            && s.contains('.')
            && !s.contains(char::is_whitespace)
            && s.len() <= 253)
            .then_some(Self(s.to_string()))
            .ok_or(())
    }
}

impl std::fmt::Display for DomainName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl AsRef<str> for DomainName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl FromStr for ResolverName {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        (!s.is_empty() && !s.contains('/') && !s.contains(char::is_whitespace))
            .then_some(Self(s.to_string()))
            .ok_or(())
    }
}

impl std::fmt::Display for ResolverName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl AsRef<str> for ResolverName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn resolver_name_rejects_empty_whitespace_and_slashes() {
        assert!("cloudflare".parse::<ResolverName>().is_ok());
        assert!("1.1.1.1".parse::<ResolverName>().is_ok());
        assert!("dns.google".parse::<ResolverName>().is_ok());
        assert!("".parse::<ResolverName>().is_err());
        assert!("bad name".parse::<ResolverName>().is_err());
        assert!("bad/name".parse::<ResolverName>().is_err());
    }
}
