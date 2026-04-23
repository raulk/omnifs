//! Wire schema for the `omnifs.provider-manifest.v1` custom section.
//!
//! The section is a concatenation of length-framed records. Each record is
//! `u32 length_le + u8 tag + u8 reserved + body_bytes`. `length_le` covers
//! the tag, reserved, and body bytes (not itself). `body_bytes` is UTF-8
//! JSON produced by `serde_json`.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

pub const MANIFEST_SECTION_NAME: &str = "omnifs.provider-manifest.v1";

pub const TAG_HANDLER: u8 = 0x01;
pub const TAG_MUTATION: u8 = 0x02;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum HandlerKindRecord {
    Dir,
    File,
    Subtree,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestCaptureRecord {
    pub name: String,
    pub type_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandlerRecord {
    pub path_template: String,
    pub handler_name: String,
    pub handler_kind: HandlerKindRecord,
    pub capture_schema: Vec<ManifestCaptureRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationRecord {
    pub path_template: String,
    pub capture_schema: Vec<ManifestCaptureRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathSegment {
    Literal(String),
    Capture {
        name: String,
        prefix: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathPattern {
    segments: Vec<PathSegment>,
    literal_count: usize,
    prefix_capture_count: usize,
    specificity: Vec<(u8, usize)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternError {
    message: String,
}

impl PatternError {
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl core::fmt::Display for PatternError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PatternError {}

impl PathPattern {
    pub fn parse(template: &str) -> Result<Self, PatternError> {
        if template == "/" {
            return Ok(Self {
                segments: Vec::new(),
                literal_count: 0,
                prefix_capture_count: 0,
                specificity: Vec::new(),
            });
        }
        if !template.starts_with('/') || template.ends_with('/') {
            return Err(pattern_error(format!("invalid path template {template:?}")));
        }

        let mut segments = Vec::new();
        let mut literal_count = 0usize;
        let mut prefix_capture_count = 0usize;
        for raw in template.split('/').skip(1) {
            if raw.starts_with("{*") {
                return Err(pattern_error(format!(
                    "rest captures are not supported in {raw:?}"
                )));
            }
            if raw.starts_with('{') && raw.ends_with('}') {
                let name = &raw[1..raw.len() - 1];
                validate_capture_name(name)?;
                segments.push(PathSegment::Capture {
                    name: name.to_string(),
                    prefix: None,
                });
                continue;
            }
            if let Some(start) = raw.find('{') {
                if !raw.ends_with('}') || raw[start + 1..raw.len() - 1].contains('{') {
                    return Err(pattern_error(format!("invalid capture segment {raw:?}")));
                }
                let prefix = &raw[..start];
                if prefix.is_empty() || prefix.contains('/') {
                    return Err(pattern_error(format!(
                        "invalid capture prefix in segment {raw:?}"
                    )));
                }
                let name = &raw[start + 1..raw.len() - 1];
                validate_capture_name(name)?;
                prefix_capture_count += 1;
                segments.push(PathSegment::Capture {
                    name: name.to_string(),
                    prefix: Some(prefix.to_string()),
                });
                continue;
            }
            literal_count += 1;
            segments.push(PathSegment::Literal(raw.to_string()));
        }

        let specificity = segments.iter().map(segment_specificity).collect();
        Ok(Self {
            segments,
            literal_count,
            prefix_capture_count,
            specificity,
        })
    }

    #[must_use]
    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }

    #[must_use]
    pub fn precedence_key(&self) -> (usize, usize, usize) {
        (
            self.literal_count,
            self.prefix_capture_count,
            self.segments.len(),
        )
    }

    #[must_use]
    pub fn matches_path(&self, path: &str) -> bool {
        let segments = split_absolute_path(path).ok();
        segments.is_some_and(|segments| {
            segments.len() == self.segments.len() && self.matches_prefix_segments(&segments)
        })
    }

    #[must_use]
    pub fn matches_parent_path(&self, path: &str) -> bool {
        let segments = split_absolute_path(path).ok();
        segments.is_some_and(|segments| {
            segments.len() + 1 == self.segments.len() && self.matches_prefix_segments(&segments)
        })
    }

    #[must_use]
    pub fn static_child(&self) -> Option<&str> {
        match self.segments.last()? {
            PathSegment::Literal(name) => Some(name),
            PathSegment::Capture { .. } => None,
        }
    }

    #[must_use]
    pub fn parent_signature(&self) -> String {
        self.segments
            .iter()
            .take(self.segments.len().saturating_sub(1))
            .map(segment_signature)
            .collect::<Vec<_>>()
            .join("/")
    }

    #[must_use]
    pub fn concrete_path_for(&self, concrete_path: &str) -> Option<String> {
        let segments = split_absolute_path(concrete_path).ok()?;
        if self.segments.len() > segments.len() || !self.matches_prefix_segments(&segments) {
            return None;
        }
        Some(join_absolute_path(&segments[..self.segments.len()]))
    }

    #[must_use]
    pub fn matches_exact_path(&self, concrete_path: &str) -> bool {
        self.concrete_path_for(concrete_path)
            .is_some_and(|matched| matched == concrete_path)
    }

    #[must_use]
    pub fn pattern_len(&self) -> usize {
        self.segments.len()
    }

    #[must_use]
    pub fn specificity(&self) -> &[(u8, usize)] {
        &self.specificity
    }

    #[must_use]
    pub fn is_ambiguous_with(&self, other: &Self) -> bool {
        self.precedence_key() == other.precedence_key()
            && self.segments.len() == other.segments.len()
            && self
                .segments
                .iter()
                .zip(other.segments.iter())
                .all(|(left, right)| segments_overlap(left, right))
    }

    fn matches_prefix_segments(&self, concrete: &[&str]) -> bool {
        self.segments
            .iter()
            .take(concrete.len())
            .zip(concrete.iter().copied())
            .all(|(pattern, actual)| match pattern {
                PathSegment::Literal(expected) => actual == expected,
                PathSegment::Capture { prefix: None, .. } => !actual.is_empty(),
                PathSegment::Capture {
                    prefix: Some(prefix),
                    ..
                } => actual
                    .strip_prefix(prefix)
                    .is_some_and(|rest| !rest.is_empty()),
            })
    }
}

#[must_use]
pub fn frame_record(tag: u8, body: &[u8]) -> Vec<u8> {
    let len = u32::try_from(body.len() + 2).expect("record body + header fits u32");
    let mut out = Vec::with_capacity(4 + body.len() + 2);
    out.extend_from_slice(&len.to_le_bytes());
    out.push(tag);
    out.push(0u8);
    out.extend_from_slice(body);
    out
}

pub fn encode_handler(rec: &HandlerRecord) -> Result<Vec<u8>, serde_json::Error> {
    let body = serde_json::to_vec(rec)?;
    Ok(frame_record(TAG_HANDLER, &body))
}

pub fn encode_mutation(rec: &MutationRecord) -> Result<Vec<u8>, serde_json::Error> {
    let body = serde_json::to_vec(rec)?;
    Ok(frame_record(TAG_MUTATION, &body))
}

pub struct ManifestRecordIter<'a> {
    rest: &'a [u8],
}

impl<'a> ManifestRecordIter<'a> {
    #[must_use]
    pub fn new(section: &'a [u8]) -> Self {
        Self { rest: section }
    }
}

#[derive(Clone, Debug)]
pub enum ManifestRecord {
    Handler(HandlerRecord),
    Mutation(MutationRecord),
    Unknown { tag: u8, body: Vec<u8> },
}

impl Iterator for ManifestRecordIter<'_> {
    type Item = Result<ManifestRecord, DecodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        if self.rest.len() < 6 {
            return Some(Err(DecodeError::Truncated));
        }
        let len_bytes: [u8; 4] = self.rest[0..4].try_into().unwrap();
        let len = u32::from_le_bytes(len_bytes) as usize;
        if len < 2 || self.rest.len() < 4 + len {
            return Some(Err(DecodeError::Truncated));
        }
        let tag = self.rest[4];
        let body = &self.rest[6..4 + len];
        self.rest = &self.rest[4 + len..];
        Some(decode_manifest_one(tag, body))
    }
}

fn decode_manifest_one(tag: u8, body: &[u8]) -> Result<ManifestRecord, DecodeError> {
    match tag {
        TAG_HANDLER => serde_json::from_slice(body)
            .map(ManifestRecord::Handler)
            .map_err(DecodeError::Json),
        TAG_MUTATION => serde_json::from_slice(body)
            .map(ManifestRecord::Mutation)
            .map_err(DecodeError::Json),
        other => Ok(ManifestRecord::Unknown {
            tag: other,
            body: body.to_vec(),
        }),
    }
}

#[derive(Debug)]
pub enum DecodeError {
    Truncated,
    Json(serde_json::Error),
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeError::Truncated => write!(f, "truncated record in provider manifest section"),
            DecodeError::Json(error) => write!(f, "json decode error: {error}"),
        }
    }
}

impl std::error::Error for DecodeError {}

fn pattern_error(message: String) -> PatternError {
    PatternError { message }
}

fn validate_capture_name(name: &str) -> Result<(), PatternError> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(pattern_error("capture names cannot be empty".to_string()));
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return Err(pattern_error(format!("invalid capture name {name:?}")));
    }
    if chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()) {
        Ok(())
    } else {
        Err(pattern_error(format!("invalid capture name {name:?}")))
    }
}

fn split_absolute_path(path: &str) -> Result<Vec<&str>, PatternError> {
    if path == "/" {
        return Ok(Vec::new());
    }
    if !path.starts_with('/') || path.ends_with('/') {
        return Err(pattern_error(format!("invalid absolute path {path:?}")));
    }
    Ok(path.split('/').skip(1).collect())
}

fn join_absolute_path(segments: &[&str]) -> String {
    if segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", segments.join("/"))
    }
}

fn segment_specificity(segment: &PathSegment) -> (u8, usize) {
    match segment {
        PathSegment::Literal(value) => (2, value.len()),
        PathSegment::Capture {
            prefix: Some(prefix),
            ..
        } => (1, prefix.len()),
        PathSegment::Capture { prefix: None, .. } => (0, 0),
    }
}

fn segment_signature(segment: &PathSegment) -> String {
    match segment {
        PathSegment::Literal(value) => format!("l:{value}"),
        PathSegment::Capture {
            prefix: Some(prefix),
            ..
        } => format!("p:{prefix}"),
        PathSegment::Capture { prefix: None, .. } => "c".to_string(),
    }
}

fn segments_overlap(left: &PathSegment, right: &PathSegment) -> bool {
    match (left, right) {
        (PathSegment::Literal(left), PathSegment::Literal(right)) => left == right,
        (
            PathSegment::Literal(_) | PathSegment::Capture { .. },
            PathSegment::Capture { prefix: None, .. },
        )
        | (
            PathSegment::Capture { prefix: None, .. },
            PathSegment::Literal(_) | PathSegment::Capture { .. },
        ) => true,
        (
            PathSegment::Literal(literal),
            PathSegment::Capture {
                prefix: Some(prefix),
                ..
            },
        )
        | (
            PathSegment::Capture {
                prefix: Some(prefix),
                ..
            },
            PathSegment::Literal(literal),
        ) => literal
            .strip_prefix(prefix)
            .is_some_and(|rest| !rest.is_empty()),
        (
            PathSegment::Capture {
                prefix: Some(left), ..
            },
            PathSegment::Capture {
                prefix: Some(right),
                ..
            },
        ) => left.starts_with(right) || right.starts_with(left),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_handler_and_mutation_records() {
        let mut bytes = encode_handler(&HandlerRecord {
            path_template: "/".to_string(),
            handler_name: "Root".to_string(),
            handler_kind: HandlerKindRecord::Dir,
            capture_schema: Vec::new(),
        })
        .unwrap();
        bytes.extend_from_slice(
            &encode_mutation(&MutationRecord {
                path_template: "/zones/{zone}".to_string(),
                capture_schema: vec![ManifestCaptureRecord {
                    name: "zone".to_string(),
                    type_name: "ZoneId".to_string(),
                }],
            })
            .unwrap(),
        );

        let records: Vec<_> = ManifestRecordIter::new(&bytes)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(records.len(), 2);
        assert!(matches!(
            &records[0],
            ManifestRecord::Handler(handler)
                if handler.path_template == "/" && handler.handler_name == "Root"
        ));
        assert!(matches!(
            &records[1],
            ManifestRecord::Mutation(mutation)
                if mutation.path_template == "/zones/{zone}"
        ));
    }

    #[test]
    fn manifest_record_iter_tolerates_unknown_tag() {
        let mut bytes = frame_record(0xEF, b"arbitrary");
        bytes.extend_from_slice(
            &encode_handler(&HandlerRecord {
                path_template: "/".to_string(),
                handler_name: "Root".to_string(),
                handler_kind: HandlerKindRecord::Dir,
                capture_schema: Vec::new(),
            })
            .unwrap(),
        );

        let mut iter = ManifestRecordIter::new(&bytes);
        match iter.next().unwrap().unwrap() {
            ManifestRecord::Unknown { tag: 0xEF, body } => {
                assert_eq!(body, b"arbitrary");
            },
            other => panic!("expected Unknown, got {other:?}"),
        }
        assert!(matches!(
            iter.next().unwrap().unwrap(),
            ManifestRecord::Handler(handler) if handler.handler_name == "Root"
        ));
    }

    #[test]
    fn path_pattern_matches_and_prefers_literals() {
        let repo = PathPattern::parse("/{owner}/{repo}").unwrap();
        let issue = PathPattern::parse("/{owner}/{repo}/_issues/_open/{number}").unwrap();
        let resolver = PathPattern::parse("/@{resolver}/{segment}").unwrap();
        let literal = PathPattern::parse("/_resolvers").unwrap();
        let capture = PathPattern::parse("/{segment}").unwrap();

        assert_eq!(
            repo.concrete_path_for("/openai/gvfs/_issues/_open/7"),
            Some("/openai/gvfs".to_string())
        );
        assert_eq!(
            issue.concrete_path_for("/openai/gvfs/_issues/_open/7/comments/1"),
            Some("/openai/gvfs/_issues/_open/7".to_string())
        );
        assert_eq!(
            resolver.concrete_path_for("/@google/example.com"),
            Some("/@google/example.com".to_string())
        );
        assert_eq!(resolver.concrete_path_for("/@"), None);
        assert!(literal.specificity() > capture.specificity());
    }
}
