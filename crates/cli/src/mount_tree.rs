//! `omnifs mount-tree` subcommand implementation.
//!
//! Reads the `omnifs.provider-manifest.v1` custom section from a provider
//! wasm file and renders views of the declared path handlers.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result, bail};
use wasmparser::{Parser, Payload};

use omnifs_mount_schema as mts;

#[allow(clippy::struct_excessive_bools)]
pub struct Views {
    pub tree: bool,
    pub paths: bool,
    pub by_type: bool,
}

impl Views {
    pub fn any_set(&self) -> bool {
        self.tree || self.paths || self.by_type
    }

    pub fn with_defaults(self) -> Self {
        if self.any_set() {
            self
        } else {
            Self {
                tree: true,
                paths: true,
                by_type: false,
            }
        }
    }
}

pub struct MountTreeData {
    pub handlers: Vec<mts::HandlerRecord>,
    pub mutations: Vec<mts::MutationRecord>,
}

pub fn read_from_wasm(path: &Path) -> Result<MountTreeData> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;

    let mut section_bytes = Vec::new();
    collect_sections(&bytes, &mut section_bytes)?;
    if section_bytes.is_empty() {
        bail!(
            "no {} custom section found in {}",
            mts::MANIFEST_SECTION_NAME,
            path.display()
        );
    }

    let mut handlers = Vec::new();
    let mut mutations = Vec::new();
    for record in mts::ManifestRecordIter::new(&section_bytes) {
        match record.context("decoding provider manifest record")? {
            mts::ManifestRecord::Handler(handler) => handlers.push(handler),
            mts::ManifestRecord::Mutation(mutation) => mutations.push(mutation),
            mts::ManifestRecord::Unknown { tag, .. } => {
                eprintln!("warning: unknown provider-manifest tag 0x{tag:02x}, skipping");
            },
        }
    }

    if handlers.is_empty() && mutations.is_empty() {
        bail!(
            "no handler or mutation records in {} custom section of {}",
            mts::MANIFEST_SECTION_NAME,
            path.display()
        );
    }

    Ok(MountTreeData {
        handlers,
        mutations,
    })
}

fn collect_sections(bytes: &[u8], out: &mut Vec<u8>) -> Result<()> {
    let mut work: Vec<(Parser, Range<usize>)> = vec![(Parser::new(0), 0..bytes.len())];

    while let Some((mut parser, range)) = work.pop() {
        let mut offset = range.start;
        while offset < range.end {
            let input = &bytes[offset..range.end];
            match parser.parse(input, true).context("parsing wasm")? {
                wasmparser::Chunk::NeedMoreData(_) => {
                    bail!("unexpected end of wasm data at offset {offset}");
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

pub fn render(data: &MountTreeData, views: &Views) -> String {
    let mut sections = Vec::new();

    if views.tree {
        sections.push(render_tree(data));
    }
    if views.paths {
        sections.push(render_paths(data));
    }
    if views.by_type {
        sections.push(render_by_type(data));
    }

    if !data.mutations.is_empty() {
        sections.push(render_mutations(data));
    }

    let mut out = sections.join("\n");
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn section_header(name: &str) -> String {
    format!("{name}\n{}\n", "=".repeat(60))
}

fn handler_kind_label(kind: &mts::HandlerKindRecord) -> &'static str {
    match kind {
        mts::HandlerKindRecord::Dir => "dir",
        mts::HandlerKindRecord::File => "file",
        mts::HandlerKindRecord::Subtree => "subtree",
    }
}

fn path_depth(path: &str) -> usize {
    if path == "/" {
        0
    } else {
        path.chars().filter(|&c| c == '/').count()
    }
}

fn path_tail(path: &str) -> &str {
    if path == "/" {
        "/"
    } else {
        path.rsplit('/').next().unwrap_or(path)
    }
}

fn render_tree(data: &MountTreeData) -> String {
    let mut handlers = data.handlers.clone();
    handlers.sort_by(|left, right| left.path_template.cmp(&right.path_template));

    let mut body = String::new();
    for handler in &handlers {
        let indent = "  ".repeat(path_depth(&handler.path_template));
        let _ = writeln!(
            body,
            "{indent}{} -> {} [{}]",
            path_tail(&handler.path_template),
            handler.handler_name,
            handler_kind_label(&handler.handler_kind),
        );
    }

    format!("{}{}", section_header("Tree"), body)
}

fn render_paths(data: &MountTreeData) -> String {
    let mut handlers = data.handlers.clone();
    handlers.sort_by(|left, right| left.path_template.cmp(&right.path_template));

    let col_width = handlers
        .iter()
        .map(|handler| handler.path_template.len())
        .max()
        .unwrap_or(0)
        + 2;

    let mut body = String::new();
    for handler in &handlers {
        let right = format!(
            "{} [{}]",
            handler.handler_name,
            handler_kind_label(&handler.handler_kind),
        );
        let _ = writeln!(body, "{:<col_width$}{right}", handler.path_template);
    }

    format!("{}{}", section_header("Paths"), body)
}

fn render_by_type(data: &MountTreeData) -> String {
    let mut groups: HashMap<&str, Vec<&mts::HandlerRecord>> = HashMap::new();
    for handler in &data.handlers {
        groups
            .entry(&handler.handler_name)
            .or_default()
            .push(handler);
    }

    let mut groups = groups.into_iter().collect::<Vec<_>>();
    groups.sort_by(|left, right| left.0.cmp(right.0));

    let col_width = groups.iter().map(|(name, _)| name.len()).max().unwrap_or(0) + 2;

    let mut body = String::new();
    for (name, handlers) in groups {
        let mut handlers = handlers;
        handlers.sort_by(|left, right| left.path_template.cmp(&right.path_template));

        let first = handlers[0];
        let first_right = format!(
            "{} [{}]",
            first.path_template,
            handler_kind_label(&first.handler_kind),
        );
        let _ = writeln!(body, "{name:<col_width$}{first_right}");

        for handler in handlers.iter().skip(1) {
            let right = format!(
                "{} [{}]",
                handler.path_template,
                handler_kind_label(&handler.handler_kind),
            );
            let _ = writeln!(body, "{:<col_width$}{right}", "");
        }
    }

    format!("{}{}", section_header("By type"), body)
}

fn render_mutations(data: &MountTreeData) -> String {
    let mut mutations = data.mutations.clone();
    mutations.sort_by(|left, right| left.path_template.cmp(&right.path_template));

    let mut body = String::new();
    for mutation in &mutations {
        let _ = writeln!(body, "{}", mutation.path_template);
    }

    format!("{}{}", section_header("Mutations"), body)
}
