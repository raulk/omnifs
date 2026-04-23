//! Git operations via the `gix` crate.
//!
//! Implements provider git callouts. Today the host supports only
//! `open_repo`, which clones a remote if needed and returns a tree-ref
//! handle the subtree handoff resolves to a filesystem path. Tree
//! traversal and blob reads run through FUSE bind-mount reads of the
//! clone directory, not through the WIT.

use crate::runtime::capability::CapabilityChecker;
use crate::runtime::cloner::GitCloner;
use crate::runtime::executor::{CalloutResponse, ErrorKind};
use dashmap::DashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

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

    pub fn open_repo(&self, cache_key: &str, clone_url: &str) -> CalloutResponse {
        if let Err(e) = self.capability.check_git_url(clone_url) {
            return CalloutResponse::Error {
                kind: ErrorKind::Denied,
                message: e.to_string(),
                retryable: false,
            };
        }

        let cache_path = match self.cloner.clone_if_needed(cache_key, clone_url) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(cache_key, clone_url, error = %e, "clone failed");
                return CalloutResponse::Error {
                    kind: ErrorKind::Network,
                    message: e.to_string(),
                    retryable: true,
                };
            },
        };

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.repos.insert(id, cache_path);
        CalloutResponse::GitRepoOpened(id)
    }

    /// Look up the local filesystem path for a `repo-id`.
    /// Used by the runtime to resolve `subtree` op-results.
    pub fn repo_path(&self, repo_id: u64) -> Option<PathBuf> {
        self.repos.get(&repo_id).map(|r| r.clone())
    }

    /// Register a local repo path directly, returning its repo ID.
    pub fn register_local(&self, path: PathBuf) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.repos.insert(id, path);
        id
    }
}
