//! Git-based blob resolution continuations.
//!
//! Handles the async sequence for resolving `_repo/` paths to actual
//! git objects: open repo -> get head -> list tree -> read blob.

use super::{dispatch, err};
use crate::Continuation;
use crate::omnifs::provider::types::*;

pub fn resume_open_repo(id: u64, tree_path: &str, result: &SingleEffectResult) -> ProviderResponse {
    match result {
        SingleEffectResult::GitRepoOpened(info) => dispatch(
            id,
            Continuation::GettingHeadRef {
                repo_id: info.repo,
                tree_path: tree_path.to_string(),
            },
            SingleEffect::GitHeadRef(info.repo),
        ),
        SingleEffectResult::EffectError(e) => err(&format!("git open failed: {}", e.message)),
        _ => err("unexpected result"),
    }
}

pub fn resume_head_ref(
    id: u64,
    repo_id: u64,
    tree_path: &str,
    result: &SingleEffectResult,
) -> ProviderResponse {
    match result {
        SingleEffectResult::GitRef(ref_name) => dispatch(
            id,
            Continuation::ListingTree,
            SingleEffect::GitListTree(GitTreeRequest {
                repo: repo_id,
                ref_name: ref_name.clone(),
                path: tree_path.to_string(),
            }),
        ),
        SingleEffectResult::EffectError(e) => err(&format!("git head ref failed: {}", e.message)),
        _ => err("unexpected result"),
    }
}

pub fn resume_listing_tree(result: &SingleEffectResult) -> ProviderResponse {
    match result {
        SingleEffectResult::GitTreeEntries(entries) => {
            let dir_entries: Vec<DirEntry> = entries
                .iter()
                .filter(|e| e.name != ".git")
                .map(|e| {
                    let (kind, size) = match e.kind {
                        GitEntryKind::Tree => (EntryKind::Directory, None),
                        _ => (EntryKind::File, Some(4096)),
                    };
                    DirEntry {
                        name: e.name.clone(),
                        kind,
                        size,
                    }
                })
                .collect();
            ProviderResponse::Done(ActionResult::DirEntries(dir_entries))
        }
        SingleEffectResult::EffectError(e) => err(&format!("git tree list failed: {}", e.message)),
        _ => err("unexpected result"),
    }
}

pub fn resume_resolving_blob_open(
    id: u64,
    tree_path: &str,
    result: &SingleEffectResult,
) -> ProviderResponse {
    match result {
        SingleEffectResult::GitRepoOpened(info) => dispatch(
            id,
            Continuation::ResolvingBlobHead {
                repo_id: info.repo,
                tree_path: tree_path.to_string(),
            },
            SingleEffect::GitHeadRef(info.repo),
        ),
        SingleEffectResult::EffectError(e) => err(&format!("git open failed: {}", e.message)),
        _ => err("unexpected result"),
    }
}

pub fn resume_resolving_blob_head(
    id: u64,
    repo_id: u64,
    tree_path: &str,
    result: &SingleEffectResult,
) -> ProviderResponse {
    match result {
        SingleEffectResult::GitRef(ref_name) => {
            let (parent, filename) = match tree_path.rsplit_once('/') {
                Some((p, f)) => (p.to_string(), f.to_string()),
                None => (String::new(), tree_path.to_string()),
            };
            dispatch(
                id,
                Continuation::ResolvingBlobTree {
                    repo_id,
                    ref_name: ref_name.clone(),
                    parent: parent.clone(),
                    filename,
                },
                SingleEffect::GitListTree(GitTreeRequest {
                    repo: repo_id,
                    ref_name: ref_name.clone(),
                    path: parent,
                }),
            )
        }
        SingleEffectResult::EffectError(e) => err(&format!("git head ref failed: {}", e.message)),
        _ => err("unexpected result"),
    }
}

pub fn resume_resolving_blob_tree(
    id: u64,
    repo_id: u64,
    _ref_name: &str,
    _parent: &str,
    filename: &str,
    result: &SingleEffectResult,
) -> ProviderResponse {
    match result {
        SingleEffectResult::GitTreeEntries(entries) => {
            let entry = entries.iter().find(|e| e.name == filename);
            match entry {
                Some(e) if e.kind != GitEntryKind::Blob => err(&format!("{filename}: not a file")),
                Some(e) => dispatch(
                    id,
                    Continuation::ResolvingBlobRead,
                    SingleEffect::GitReadBlob(GitBlobRequest {
                        repo: repo_id,
                        oid: e.oid.clone(),
                    }),
                ),
                None => err(&format!("file not found in tree: {filename}")),
            }
        }
        SingleEffectResult::EffectError(e) => err(&format!("git tree list failed: {}", e.message)),
        _ => err("unexpected result"),
    }
}

pub fn resume_resolving_blob_read(result: &SingleEffectResult) -> ProviderResponse {
    match result {
        SingleEffectResult::GitBlobData(data) => {
            ProviderResponse::Done(ActionResult::FileContent(data.clone()))
        }
        SingleEffectResult::EffectError(e) => err(&format!("git blob read failed: {}", e.message)),
        _ => err("unexpected result"),
    }
}
