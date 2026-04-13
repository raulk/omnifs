//! Browse module: filesystem routing and async continuations.
//!
//! Routes FUSE operations to the appropriate handlers and manages the
//! async continuation state machine for GitHub API operations.

use crate::omnifs::provider::types::*;
use crate::{Continuation, SingleEffect, SingleEffectResult, with_state};

pub const MAX_CONTENT_SIZE: usize = 10 * 1024 * 1024;
const TRUNCATION_MARKER: &[u8] = b"\n[truncated at 10MB]\n";

pub const NEGATIVE_OWNER_TTL: u64 = 300;
pub const ACTIVE_REPO_TTL: u64 = 600;

mod events;
mod files;
mod git;
mod resources;
mod routing;

// Re-exports
pub use events::timer_tick;
pub use routing::{list_entries, read_file, resolve_entry};

// Re-export types needed by submodules
pub(crate) use crate::CachedRepoListMode;

/// Path structure:
///   ""                         -> list owners (from cache)
///   "<owner>"                  -> list repos
///   "<owner>/<repo>"           -> list namespace dirs: _repo, _issues, _prs, _actions
///   "<owner>/<repo>/_repo/..." -> git tree browsing
///   "<owner>/<repo>/_issues/_open/<num>/..." -> issue files
///   "<owner>/<repo>/_prs/_open/<num>/..."    -> PR files
///   "<owner>/<repo>/_actions/runs/<id>/..."  -> action run files
#[allow(clippy::needless_pass_by_value)] // EffectResult is owned from the WIT trait boundary
pub fn resume(id: u64, effect_outcome: EffectResult) -> ProviderResponse {
    let continuation = match with_state(|state| state.pending.remove(&id)) {
        Ok(Some(c)) => c,
        Ok(None) => return err("no pending continuation"),
        Err(e) => return err(&e),
    };

    let result = match &effect_outcome {
        EffectResult::Single(r) => r,
        EffectResult::Batch(results) if !results.is_empty() => &results[0],
        EffectResult::Batch(_) => return err("empty batch result"),
    };

    match continuation {
        Continuation::ListingCachedRepos { path, mode } => {
            resources::resume_cached_repos(&path, &mode, result)
        }
        Continuation::FetchingOwnerProfile {
            path,
            is_org_fallback,
        } => resources::resume_owner_profile(id, &path, is_org_fallback, result),
        Continuation::FetchingRepoPages { path } => {
            resources::resume_repo_pages(id, &path, &effect_outcome)
        }
        Continuation::FetchingFirstPage {
            path,
            is_org_fallback,
        } => resources::resume_list_first_page(id, &path, is_org_fallback, result),
        Continuation::FetchingRemainingPages {
            path,
            first_page_items,
        } => resources::resume_list_remaining(id, &path, first_page_items, &effect_outcome),
        Continuation::FetchingResource { path } => files::resume_resource(&path, result),
        Continuation::ValidatingRepo { path } => files::resume_validating_repo(id, &path, result),
        Continuation::ValidatingResource { path, name } => {
            files::resume_validating_resource(&path, &name, result)
        }
        Continuation::FetchingComments { path } => files::resume_comments(&path, result),
        Continuation::DisowningRepo => git::resume_open_repo_disown(id, result),
        Continuation::FetchingDiff { path } => events::resume_diff(&path, result),
        Continuation::FetchingRunLog { path } => events::resume_run_log(&path, result),
        Continuation::FetchingEvents { repos, invalidation_count } => events::resume_events(&repos, invalidation_count, &effect_outcome),
    }
}

// --- Shared helpers ---

pub(crate) fn err(msg: &str) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::Err(msg.to_string()))
}

pub(crate) fn dispatch(id: u64, cont: Continuation, effect: SingleEffect) -> ProviderResponse {
    match with_state(|state| {
        state.pending.insert(id, cont);
    }) {
        Ok(()) => ProviderResponse::Effect(effect),
        Err(e) => err(&e),
    }
}

pub(crate) fn cache_only() -> bool {
    with_state(|state| state.cache_only).unwrap_or(false)
}

pub(crate) fn enter_cache_only() {
    let _ = with_state(|state| {
        state.cache_only = true;
    });
}

pub(crate) fn is_unauthorized(result: &SingleEffectResult) -> bool {
    matches!(
        result,
        SingleEffectResult::HttpResponse(HttpResponse { status: 401, .. })
    )
}

pub(crate) fn touch_repo(owner: &str, repo: &str) {
    if !crate::path::is_safe_segment(owner) || !crate::path::is_safe_segment(repo) {
        return;
    }
    let _ = with_state(|state| {
        let tick = state.cache.current_tick();
        state.active_repos.insert(format!("{owner}/{repo}"), tick);
    });
}

pub(crate) fn extract_http_body(result: &SingleEffectResult) -> Result<&[u8], ProviderResponse> {
    match result {
        SingleEffectResult::HttpResponse(resp) => {
            check_rate_limit(resp);
            if resp.status >= 400 {
                Err(err(&format!("API error: {}", resp.status)))
            } else {
                Ok(&resp.body)
            }
        }
        SingleEffectResult::EffectError(e) => Err(err(&format!("effect error: {}", e.message))),
        _ => Err(err("unexpected effect result type")),
    }
}

/// Track X-RateLimit-Remaining from GitHub API responses. Stores the
/// value in `ProviderState` and logs a warning via the host when low.
pub(crate) fn check_rate_limit(resp: &HttpResponse) {
    let Some(remaining) = http_header_value(resp, "x-ratelimit-remaining")
        .and_then(|value| value.parse::<u32>().ok())
    else {
        return;
    };

    let _ = with_state(|state| {
        state.rate_limit_remaining = Some(remaining);
    });

    let limit =
        http_header_value(resp, "x-ratelimit-limit").and_then(|value| value.parse::<u32>().ok());
    if should_warn_rate_limit(remaining, limit) {
        let resource = http_header_value(resp, "x-ratelimit-resource").unwrap_or("unknown");
        let message = match limit {
            Some(limit) => {
                format!("GitHub API {resource} rate limit low: {remaining}/{limit} remaining")
            }
            None => format!("GitHub API {resource} rate limit low: {remaining} remaining"),
        };
        crate::omnifs::provider::log::log(&LogEntry {
            level: LogLevel::Warn,
            message,
        });
    }
}

fn http_header_value<'a>(resp: &'a HttpResponse, name: &str) -> Option<&'a str> {
    resp.headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str())
}

fn should_warn_rate_limit(remaining: u32, limit: Option<u32>) -> bool {
    remaining <= rate_limit_warning_threshold(limit)
}

fn rate_limit_warning_threshold(limit: Option<u32>) -> u32 {
    limit.map_or(99, |limit| (limit / 10).clamp(1, 100))
}

pub(crate) fn truncate_content(mut data: Vec<u8>) -> Vec<u8> {
    if data.len() <= MAX_CONTENT_SIZE {
        return data;
    }
    data.truncate(MAX_CONTENT_SIZE.saturating_sub(TRUNCATION_MARKER.len()));
    data.extend_from_slice(TRUNCATION_MARKER);
    data
}

pub(crate) fn dir_entry(name: &str) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::DirEntryOption(Some(DirEntry {
        name: name.to_string(),
        kind: EntryKind::Directory,
        size: None,
        projected_files: None,
    })))
}

pub(crate) fn file_entry(name: &str) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::DirEntryOption(Some(DirEntry {
        name: name.to_string(),
        kind: EntryKind::File,
        size: Some(4096),
        projected_files: None,
    })))
}

// Re-export helper functions that submodules need from resources module
pub(crate) use files::{
    list_cached_comments, serve_comment_file, serve_resource_file, serve_run_file,
};
pub(crate) use resources::{
    finalize_cached_resource_list, finalize_cached_runs_list, finalize_search_results,
};
