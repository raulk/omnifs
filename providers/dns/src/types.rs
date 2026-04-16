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

pub(crate) fn is_domain_like(s: &str) -> bool {
    s.contains('.') && !s.contains(char::is_whitespace) && s.len() <= 253
}

pub(crate) fn is_ip_addr(s: &str) -> bool {
    s.parse::<IpAddr>().is_ok()
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
}
