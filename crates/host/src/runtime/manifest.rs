use std::ops::Range;
use std::path::Path;

use omnifs_mount_schema as mts;
use wasmparser::{Parser, Payload};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclaredHandler {
    pub mount_id: String,
    pub mount_name: String,
    pub kind: DeclaredHandlerKind,
    pattern: mts::PathPattern,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeclaredHandlerKind {
    Dir,
    File,
    Subtree,
}

impl DeclaredHandler {
    fn new(record: mts::HandlerRecord) -> Result<Self, String> {
        let pattern = mts::PathPattern::parse(&record.path_template)
            .map_err(|error| error.message().to_string())?;
        let kind = match record.handler_kind {
            mts::HandlerKindRecord::Dir => DeclaredHandlerKind::Dir,
            mts::HandlerKindRecord::File => DeclaredHandlerKind::File,
            mts::HandlerKindRecord::Subtree => DeclaredHandlerKind::Subtree,
        };
        Ok(Self {
            mount_id: record.path_template,
            mount_name: record.handler_name,
            kind,
            pattern,
        })
    }

    pub fn concrete_path_for(&self, concrete_path: &str) -> Option<String> {
        self.pattern.concrete_path_for(concrete_path)
    }

    pub fn matches_exact_path(&self, concrete_path: &str) -> bool {
        self.pattern.matches_exact_path(concrete_path)
    }

    pub fn pattern_len(&self) -> usize {
        self.pattern.pattern_len()
    }

    pub fn specificity(&self) -> &[(u8, usize)] {
        self.pattern.specificity()
    }
}

pub fn read_declared_handlers_from_wasm(path: &Path) -> Result<Vec<DeclaredHandler>, String> {
    let bytes =
        std::fs::read(path).map_err(|error| format!("reading {}: {error}", path.display()))?;
    let mut section_bytes = Vec::new();
    collect_sections(&bytes, &mut section_bytes)?;
    if section_bytes.is_empty() {
        return Ok(Vec::new());
    }

    let mut handlers = Vec::new();
    for record in mts::ManifestRecordIter::new(&section_bytes) {
        match record.map_err(|error| format!("decoding provider manifest record: {error}"))? {
            mts::ManifestRecord::Handler(handler) => handlers.push(DeclaredHandler::new(handler)?),
            mts::ManifestRecord::Mutation(_) | mts::ManifestRecord::Unknown { .. } => {},
        }
    }

    Ok(handlers)
}

fn collect_sections(bytes: &[u8], out: &mut Vec<u8>) -> Result<(), String> {
    let mut work: Vec<(Parser, Range<usize>)> = vec![(Parser::new(0), 0..bytes.len())];

    while let Some((mut parser, range)) = work.pop() {
        let mut offset = range.start;
        while offset < range.end {
            let input = &bytes[offset..range.end];
            match parser
                .parse(input, true)
                .map_err(|error| format!("parsing wasm: {error}"))?
            {
                wasmparser::Chunk::NeedMoreData(_) => {
                    return Err(format!("unexpected end of wasm data at offset {offset}"));
                },
                wasmparser::Chunk::Parsed { consumed, payload } => {
                    offset += consumed;
                    match payload {
                        Payload::CustomSection(reader)
                            if reader.name() == mts::MANIFEST_SECTION_NAME =>
                        {
                            out.extend_from_slice(reader.data());
                        },
                        Payload::ModuleSection {
                            parser: sub,
                            unchecked_range,
                            ..
                        }
                        | Payload::ComponentSection {
                            parser: sub,
                            unchecked_range,
                            ..
                        } => {
                            offset = offset.max(unchecked_range.end);
                            work.push((sub, unchecked_range));
                        },
                        Payload::End(_) => break,
                        _ => {},
                    }
                },
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{DeclaredHandler, DeclaredHandlerKind};
    use omnifs_mount_schema::{HandlerKindRecord, HandlerRecord};

    #[test]
    fn declared_handler_matches_capture_patterns_and_returns_concrete_path() {
        let repo = DeclaredHandler::new(HandlerRecord {
            path_template: "/{owner}/{repo}".to_string(),
            handler_name: "Repo".to_string(),
            handler_kind: HandlerKindRecord::Dir,
            capture_schema: Vec::new(),
        })
        .unwrap();
        let issue = DeclaredHandler::new(HandlerRecord {
            path_template: "/{owner}/{repo}/_issues/_open/{number}".to_string(),
            handler_name: "Issue".to_string(),
            handler_kind: HandlerKindRecord::Dir,
            capture_schema: Vec::new(),
        })
        .unwrap();
        let resolver = DeclaredHandler::new(HandlerRecord {
            path_template: "/@{resolver}/{segment}".to_string(),
            handler_name: "ResolverSegment".to_string(),
            handler_kind: HandlerKindRecord::Dir,
            capture_schema: Vec::new(),
        })
        .unwrap();

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
        assert_eq!(repo.concrete_path_for("/_resolvers"), None);
        assert_eq!(resolver.concrete_path_for("/@"), None);
    }

    #[test]
    fn declared_handler_specificity_prefers_literals_over_captures() {
        let literal = DeclaredHandler::new(HandlerRecord {
            path_template: "/_resolvers".to_string(),
            handler_name: "Resolvers".to_string(),
            handler_kind: HandlerKindRecord::File,
            capture_schema: Vec::new(),
        })
        .unwrap();
        let prefixed = DeclaredHandler::new(HandlerRecord {
            path_template: "/@{resolver}".to_string(),
            handler_name: "ResolverRoot".to_string(),
            handler_kind: HandlerKindRecord::Dir,
            capture_schema: Vec::new(),
        })
        .unwrap();
        let capture = DeclaredHandler::new(HandlerRecord {
            path_template: "/{segment}".to_string(),
            handler_name: "Segment".to_string(),
            handler_kind: HandlerKindRecord::Dir,
            capture_schema: Vec::new(),
        })
        .unwrap();

        assert_eq!(literal.kind, DeclaredHandlerKind::File);
        assert!(literal.specificity() > capture.specificity());
        assert!(prefixed.specificity() > capture.specificity());
    }
}
