//! Git operations via the `gix` crate.
//!
//! Implements provider Git effects: open repo, list tree, read blob,
//! head ref, and list cached repos. Uses `GitCloner` for on-demand cloning.

use crate::runtime::capability::CapabilityChecker;
use crate::runtime::cloner::GitCloner;
use crate::runtime::executor::{
    EffectResponse, ErrorKind, GitCachedRepoData, GitEntryKind, GitTreeEntryData,
};
use dashmap::DashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, thiserror::Error)]
pub(crate) enum GitError {
    #[error("failed to open repository")]
    Open(#[from] Box<gix::open::Error>),
    #[error("ref resolution failed: {context}")]
    Ref { context: String },
    #[error("tree traversal failed: {context}")]
    Tree { context: String },
    #[error("blob read failed: {context}")]
    Blob { context: String },
    #[error("path not found: {0}")]
    PathNotFound(String),
    #[error("repo not opened")]
    RepoNotOpened,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub struct GitExecutor {
    cloner: Arc<GitCloner>,
    capability: Arc<CapabilityChecker>,
    repos: DashMap<u64, PathBuf>,
    next_id: AtomicU64,
}

impl GitExecutor {
    pub fn new(cloner: Arc<GitCloner>, capability: Arc<CapabilityChecker>) -> Self {
        Self {
            cloner,
            capability,
            repos: DashMap::new(),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn open_repo(&self, cache_key: &str, clone_url: &str) -> EffectResponse {
        if let Err(e) = self.capability.check_git_url(clone_url) {
            return EffectResponse::Error {
                kind: ErrorKind::Denied,
                message: e.to_string(),
                retryable: false,
            };
        }

        let cache_path = match self.cloner.clone_if_needed(cache_key, clone_url) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(cache_key, clone_url, error = %e, "clone failed");
                return EffectResponse::Error {
                    kind: ErrorKind::Network,
                    message: e.to_string(),
                    retryable: true,
                };
            }
        };

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.repos.insert(id, cache_path);
        EffectResponse::GitRepoOpened(id)
    }

    fn get_repo_path(&self, repo_id: u64) -> Result<PathBuf, GitError> {
        self.repos
            .get(&repo_id)
            .map(|r| r.clone())
            .ok_or(GitError::RepoNotOpened)
    }

    /// Look up the local filesystem path for a `repo-id`.
    /// Used by `EffectRuntime` to resolve `RepoTree` action results.
    pub fn repo_path(&self, repo_id: u64) -> Option<PathBuf> {
        self.repos.get(&repo_id).map(|r| r.clone())
    }

    pub fn list_tree(&self, repo_id: u64, ref_name: &str, path: &str) -> EffectResponse {
        let repo_path = match self.get_repo_path(repo_id) {
            Ok(p) => p,
            Err(e) => {
                return EffectResponse::Error {
                    kind: ErrorKind::NotFound,
                    message: e.to_string(),
                    retryable: false,
                };
            }
        };

        match Self::read_tree_entries(&repo_path, ref_name, path) {
            Ok(entries) => EffectResponse::GitTreeEntries(entries),
            Err(e) => EffectResponse::Error {
                kind: ErrorKind::Internal,
                message: e.to_string(),
                retryable: false,
            },
        }
    }

    pub fn read_blob(&self, repo_id: u64, oid: &str) -> EffectResponse {
        let repo_path = match self.get_repo_path(repo_id) {
            Ok(p) => p,
            Err(e) => {
                return EffectResponse::Error {
                    kind: ErrorKind::NotFound,
                    message: e.to_string(),
                    retryable: false,
                };
            }
        };

        match Self::read_blob_content(&repo_path, oid) {
            Ok(data) => EffectResponse::GitBlobData(data),
            Err(e) => EffectResponse::Error {
                kind: ErrorKind::Internal,
                message: e.to_string(),
                retryable: false,
            },
        }
    }

    pub fn head_ref(&self, repo_id: u64) -> EffectResponse {
        let repo_path = match self.get_repo_path(repo_id) {
            Ok(p) => p,
            Err(e) => {
                return EffectResponse::Error {
                    kind: ErrorKind::NotFound,
                    message: e.to_string(),
                    retryable: false,
                };
            }
        };

        match Self::read_head_ref(&repo_path) {
            Ok(ref_name) => EffectResponse::GitRef(ref_name),
            Err(e) => EffectResponse::Error {
                kind: ErrorKind::Internal,
                message: e.to_string(),
                retryable: false,
            },
        }
    }

    pub fn list_cached_repos(&self, prefix: Option<&str>) -> EffectResponse {
        match list_cached_repos_generic(self.cloner.cache_dir(), prefix) {
            Ok(repos) => EffectResponse::GitCachedRepos(repos),
            Err(e) => EffectResponse::Error {
                kind: ErrorKind::Internal,
                message: e.to_string(),
                retryable: false,
            },
        }
    }

    /// Register a local repo path directly, returning its repo ID.
    pub fn register_local(&self, path: PathBuf) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.repos.insert(id, path);
        id
    }

    fn read_tree_entries(
        repo_path: &Path,
        ref_name: &str,
        path: &str,
    ) -> Result<Vec<GitTreeEntryData>, GitError> {
        let repo = gix::open(repo_path).map_err(Box::new)?;

        let reference = repo.find_reference(ref_name).map_err(|e| GitError::Ref {
            context: format!("find {ref_name}: {e}"),
        })?;
        let commit = reference
            .into_fully_peeled_id()
            .map_err(|e| GitError::Ref {
                context: format!("peel {ref_name}: {e}"),
            })?;
        let commit_obj = commit
            .object()
            .map_err(|e| GitError::Tree {
                context: format!("read commit: {e}"),
            })?
            .try_into_commit()
            .map_err(|e| GitError::Tree {
                context: format!("not a commit: {e}"),
            })?;
        let tree = commit_obj.tree().map_err(|e| GitError::Tree {
            context: format!("get tree: {e}"),
        })?;

        let target_tree = if path.is_empty() {
            tree
        } else {
            let entry = tree
                .lookup_entry_by_path(path)
                .map_err(|e| GitError::Tree {
                    context: format!("lookup {path}: {e}"),
                })?
                .ok_or_else(|| GitError::PathNotFound(path.to_string()))?;
            entry
                .object()
                .map_err(|e| GitError::Tree {
                    context: format!("read object: {e}"),
                })?
                .try_into_tree()
                .map_err(|e| GitError::Tree {
                    context: format!("not a tree: {e}"),
                })?
        };

        target_tree
            .iter()
            .map(|entry| {
                let entry = entry.map_err(|e| GitError::Tree {
                    context: format!("iter: {e}"),
                })?;
                let kind = match entry.mode().kind() {
                    gix::object::tree::EntryKind::Tree => GitEntryKind::Tree,
                    gix::object::tree::EntryKind::Commit => GitEntryKind::Commit,
                    gix::object::tree::EntryKind::Link
                    | gix::object::tree::EntryKind::Blob
                    | gix::object::tree::EntryKind::BlobExecutable => GitEntryKind::Blob,
                };
                Ok(GitTreeEntryData {
                    name: entry.filename().to_string(),
                    mode: u32::from(entry.mode().value()),
                    oid: entry.oid().to_string(),
                    kind,
                })
            })
            .collect()
    }

    fn read_blob_content(repo_path: &Path, oid: &str) -> Result<Vec<u8>, GitError> {
        let repo = gix::open(repo_path).map_err(Box::new)?;
        let id = gix::ObjectId::from_hex(oid.as_bytes()).map_err(|e| GitError::Blob {
            context: format!("parse oid: {e}"),
        })?;
        let object = repo.find_object(id).map_err(|e| GitError::Blob {
            context: format!("find object: {e}"),
        })?;
        Ok(object.data.clone())
    }

    fn read_head_ref(repo_path: &Path) -> Result<String, GitError> {
        let repo = gix::open(repo_path).map_err(Box::new)?;
        let head = repo.head_ref().map_err(|e| GitError::Ref {
            context: format!("head ref: {e}"),
        })?;
        match head {
            Some(r) => Ok(r.name().as_bstr().to_string()),
            None => Ok("HEAD".to_string()),
        }
    }
}

/// Walk the cache directory tree and collect all repos (directories containing .git).
/// Returns cache keys as relative paths from the cache root.
/// If prefix is Some, only return entries whose cache key starts with that prefix.
fn list_cached_repos_generic(
    cache_root: &Path,
    prefix: Option<&str>,
) -> Result<Vec<GitCachedRepoData>, GitError> {
    if !cache_root.exists() {
        return Ok(Vec::new());
    }

    let mut repos = Vec::new();
    walk_cache_dir(cache_root, cache_root, &mut repos)?;

    if let Some(prefix) = prefix {
        repos.retain(|r| r.cache_key.starts_with(prefix));
    }

    repos.sort_by(|a, b| a.cache_key.cmp(&b.cache_key));
    Ok(repos)
}

fn walk_cache_dir(
    root: &Path,
    current: &Path,
    repos: &mut Vec<GitCachedRepoData>,
) -> Result<(), GitError> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        if path.join(".git").is_dir() {
            if let Ok(rel) = path.strip_prefix(root) {
                let cache_key = rel
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                repos.push(GitCachedRepoData { cache_key });
            }
        } else {
            walk_cache_dir(root, &path, repos)?;
        }
    }
    Ok(())
}
