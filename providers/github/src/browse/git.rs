//! Git repo disowning.
//!
//! When a `_repo/` path is accessed, the provider triggers a clone
//! and returns `DisownedTree` to hand the subtree to FUSE passthrough.

use super::err;
use crate::omnifs::provider::types::*;

/// Resume after `OpenRepo`. Returns `DisownedTree` to hand the subtree to FUSE passthrough.
pub fn resume_open_repo_disown(_id: u64, result: &SingleEffectResult) -> ProviderResponse {
    match result {
        SingleEffectResult::GitRepoOpened(info) => {
            ProviderResponse::Done(ActionResult::DisownedTree(info.tree))
        }
        SingleEffectResult::EffectError(e) => err(&format!("git open failed: {}", e.message)),
        _ => err("unexpected result"),
    }
}
