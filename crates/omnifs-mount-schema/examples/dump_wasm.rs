//! Example: read the `omnifs.provider-manifest.v1` custom section from a
//! wasm file and print a summary of handler and mutation records.
//!
//! Run with:
//!   cargo run -p omnifs-mount-schema --example dump_wasm -- \
//!     target/wasm32-wasip2/debug/omnifs_provider_github.wasm

use std::env;
use std::fs;
use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use omnifs_mount_schema as mts;
use wasmparser::{Parser, Payload};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let path = args
        .get(1)
        .ok_or_else(|| anyhow!("usage: dump_wasm <path-to-wasm>"))?;
    let bytes = fs::read(Path::new(path)).with_context(|| format!("reading {path}"))?;

    let mut section_bytes = Vec::new();
    collect_sections(&bytes, &mut section_bytes)?;
    if section_bytes.is_empty() {
        bail!("no {} custom section found", mts::MANIFEST_SECTION_NAME);
    }

    let mut handlers = 0usize;
    let mut mutations = 0usize;
    let mut unknown = 0usize;
    for record in mts::ManifestRecordIter::new(&section_bytes) {
        match record? {
            mts::ManifestRecord::Handler(handler) => {
                handlers += 1;
                println!(
                    "handler: {} [{}] -> {}",
                    handler.path_template,
                    match handler.handler_kind {
                        mts::HandlerKindRecord::Dir => "dir",
                        mts::HandlerKindRecord::File => "file",
                        mts::HandlerKindRecord::Subtree => "subtree",
                    },
                    handler.handler_name,
                );
            },
            mts::ManifestRecord::Mutation(mutation) => {
                mutations += 1;
                println!("mutation: {}", mutation.path_template);
            },
            mts::ManifestRecord::Unknown { tag, .. } => {
                unknown += 1;
                eprintln!("unknown tag 0x{tag:02x}");
            },
        }
    }

    println!("summary: handlers={handlers} mutations={mutations} unknown={unknown}");
    Ok(())
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
