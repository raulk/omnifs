//! Common response helpers for providers.

use crate::Op;
use crate::error::ProviderError;
use crate::omnifs::provider::types::{ActionResult, DirEntry, EntryKind, ProviderResponse};

pub fn err(error: impl Into<ProviderError>) -> ProviderResponse {
    error.into().into()
}

pub fn dir_entry(name: impl Into<String>) -> ProviderResponse {
    let name = name.into();
    ProviderResponse::Done(ActionResult::DirEntryOption(Some(DirEntry {
        name,
        kind: EntryKind::Directory,
        size: None,
        projected_files: None,
    })))
}

pub fn file_entry(name: impl Into<String>) -> ProviderResponse {
    let name = name.into();
    ProviderResponse::Done(ActionResult::DirEntryOption(Some(DirEntry {
        name,
        kind: EntryKind::File,
        size: Some(4096),
        projected_files: None,
    })))
}

pub fn mk_dir(name: impl Into<String>) -> DirEntry {
    DirEntry {
        name: name.into(),
        kind: EntryKind::Directory,
        size: None,
        projected_files: None,
    }
}

pub fn mk_file(name: impl Into<String>) -> DirEntry {
    DirEntry {
        name: name.into(),
        kind: EntryKind::File,
        size: Some(4096),
        projected_files: None,
    }
}

/// Route helper for directory-like nodes.
/// - Lookup: returns a directory dir-entry
/// - List: invokes `list` and returns its response
/// - Read: returns a "not a file" error
pub fn dir_only<F>(op: Op, name: impl Into<String>, list: F) -> Option<ProviderResponse>
where
    F: FnOnce(u64) -> ProviderResponse,
{
    dir_only_with(op, name, list, |_| {
        Some(err(ProviderError::invalid_input("not a file")))
    })
}

/// Route helper for file-like nodes.
/// - Lookup: returns a file dir-entry
/// - List: returns a "not a directory" error
/// - Read: invokes `read` and returns its response
pub fn file_only<F>(op: Op, name: impl Into<String>, read: F) -> Option<ProviderResponse>
where
    F: FnOnce(u64) -> ProviderResponse,
{
    file_only_with(op, name, read, |_| {
        err(ProviderError::invalid_input("not a directory"))
    })
}

/// Route helper for directory-like nodes where reads are not supported and should be
/// treated as "not found" rather than an explicit operation error.
pub fn dir_only_no_read<F>(op: Op, name: impl Into<String>, list: F) -> Option<ProviderResponse>
where
    F: FnOnce(u64) -> ProviderResponse,
{
    dir_only_with(op, name, list, |_| None)
}

/// Directory helper with custom read behavior.
pub fn dir_only_with<F, R>(
    op: Op,
    name: impl Into<String>,
    list: F,
    read: R,
) -> Option<ProviderResponse>
where
    F: FnOnce(u64) -> ProviderResponse,
    R: FnOnce(u64) -> Option<ProviderResponse>,
{
    let name = name.into();
    match op {
        Op::Lookup(_) => Some(dir_entry(name)),
        Op::List(id) => Some(list(id)),
        Op::Read(id) => read(id),
    }
}

/// File helper with custom list behavior.
pub fn file_only_with<R, L>(
    op: Op,
    name: impl Into<String>,
    read: R,
    list: L,
) -> Option<ProviderResponse>
where
    R: FnOnce(u64) -> ProviderResponse,
    L: FnOnce(u64) -> ProviderResponse,
{
    let name = name.into();
    match op {
        Op::Lookup(_) => Some(file_entry(name)),
        Op::List(id) => Some(list(id)),
        Op::Read(id) => Some(read(id)),
    }
}
