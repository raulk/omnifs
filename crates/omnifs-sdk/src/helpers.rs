//! Common response helpers for providers.

use crate::omnifs::provider::types::{ActionResult, DirEntry, EntryKind, ProviderResponse};

pub fn err(msg: &str) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::Err(msg.to_string()))
}

pub fn dir_entry(name: &str) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::DirEntryOption(Some(DirEntry {
        name: name.to_string(),
        kind: EntryKind::Directory,
        size: None,
        projected_files: None,
    })))
}

pub fn file_entry(name: &str) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::DirEntryOption(Some(DirEntry {
        name: name.to_string(),
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
