//! Individual file content serving continuations.
//!
//! Handles fetching and serving specific resource files (title, body,
//! comments, diffs) with stale-on-error caching.

use super::{dispatch_or_err, enter_cache_only, err, get_cached, is_unauthorized};
use crate::Continuation;
use crate::api;
use crate::path::FsPath;
use crate::types::{RepoId, ResourceFile, ResourceKind, RunFile, github_owner_cache_prefix};
use crate::with_state;
use omnifs_sdk::prelude::*;

pub fn resume_resource(path: &str, result: &SingleEffectResult) -> ProviderResponse {
    match FsPath::parse(path) {
        Some(FsPath::ResourceFile {
            owner,
            repo,
            kind,
            number,
            file,
            ..
        }) => {
            let api_resource = kind.api_path();
            let cache_key =
                RepoId::new(owner, repo).cache_path(&format!("{api_resource}/{number}"));

            // Stale-on-error: serve cached data when the API request fails.
            let Ok(body) = super::extract_http_body(result) else {
                if let Ok(Some(data)) = get_cached(&cache_key) {
                    return serve_resource_file(&data, file);
                }
                return err(ProviderError::not_found("API error and no cached data"));
            };

            let _ = with_state(|state| state.cache.set(cache_key, body.to_vec()));
            serve_resource_file(body, file)
        }
        Some(FsPath::ActionRunFile {
            owner,
            repo,
            run_id,
            file,
        }) => {
            let cache_key =
                RepoId::new(owner, repo).cache_path(&format!("actions/runs/{run_id}"));

            // Stale-on-error: serve cached data when the API request fails.
            let Ok(body) = super::extract_http_body(result) else {
                if let Ok(Some(data)) = get_cached(&cache_key) {
                    return serve_run_file(&data, file);
                }
                return err(ProviderError::not_found("API error and no cached data"));
            };

            let _ = with_state(|state| state.cache.set(cache_key, body.to_vec()));
            serve_run_file(body, file)
        }
        _ => err(ProviderError::internal("invalid resource path")),
    }
}

pub fn serve_resource_file(body: &[u8], file: ResourceFile) -> ProviderResponse {
    let json = match api::parse_json(body) {
        Ok(j) => j,
        Err(e) => return err(e),
    };
    let content = match file {
        ResourceFile::Title => extract_str(&json, "title"),
        ResourceFile::Body => extract_str(&json, "body"),
        ResourceFile::State => extract_str(&json, "state"),
        ResourceFile::User => {
            json.get("user")
                .and_then(|u| u.get("login"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
                + "\n"
        }
        ResourceFile::Diff => {
            return err(ProviderError::internal("diff should be handled separately"));
        }
    };
    ProviderResponse::Done(ActionResult::FileContent(content.into_bytes()))
}

pub fn serve_run_file(body: &[u8], file: RunFile) -> ProviderResponse {
    let json = match api::parse_json(body) {
        Ok(j) => j,
        Err(e) => return err(e),
    };
    let content = match file {
        RunFile::Status => extract_str(&json, "status"),
        RunFile::Conclusion => {
            json.get("conclusion")
                .and_then(|v| v.as_str())
                .unwrap_or("pending")
                .to_string()
                + "\n"
        }
        RunFile::Log => return err(ProviderError::internal("log should be handled separately")),
    };
    ProviderResponse::Done(ActionResult::FileContent(content.into_bytes()))
}

pub fn resume_validating_repo(
    id: u64,
    path: &str,
    result: &SingleEffectResult,
) -> ProviderResponse {
    let fs_path = FsPath::parse(path);
    if is_unauthorized(result) {
        enter_cache_only();
        if let Some(FsPath::Repo { owner, .. }) = &fs_path {
            return dispatch_or_err(
                id,
                Continuation::ListingCachedRepos {
                    path: path.to_string(),
                    mode: super::CachedRepoListMode::ValidateRepo,
                },
                SingleEffect::GitListCachedRepos(GitCacheListRequest {
                    prefix: Some(github_owner_cache_prefix(owner)),
                }),
            );
        }
    }

    let status = match result {
        SingleEffectResult::HttpResponse(resp) => resp.status,
        SingleEffectResult::EffectError(e) => return err(ProviderError::from_effect_error(e)),
        _ => return err(ProviderError::internal("unexpected result")),
    };

    let name = fs_path.as_ref().and_then(FsPath::repo).unwrap_or("");

    if status == 404 {
        ProviderResponse::Done(ActionResult::DirEntryOption(None))
    } else if status >= 400 {
        err(ProviderError::from_http_status(status))
    } else {
        dir_entry(name)
    }
}

pub fn resume_validating_resource(
    path: &str,
    name: &str,
    result: &SingleEffectResult,
) -> ProviderResponse {
    // Extract owner/repo/kind/number from the path (Resource or deeper variant).
    let resource_info: Option<(&str, &str, ResourceKind, &str)> =
        FsPath::parse(path).and_then(|p| match p {
            FsPath::Resource {
                owner,
                repo,
                kind,
                number,
                ..
            } => Some((owner, repo, kind, number)),
            _ => None,
        });

    if is_unauthorized(result) {
        enter_cache_only();
        if let Some((owner, repo, kind, number)) = resource_info {
            let cache_key =
                RepoId::new(owner, repo).cache_path(&format!("{}/{number}", kind.api_path()));
            let cached = get_cached(&cache_key)
                .map(|entry| entry.is_some())
                .unwrap_or(false);
            if cached {
                return dir_entry(name);
            }
        }
        return ProviderResponse::Done(ActionResult::DirEntryOption(None));
    }

    match result {
        SingleEffectResult::HttpResponse(resp) if resp.status == 404 => {
            ProviderResponse::Done(ActionResult::DirEntryOption(None))
        }
        SingleEffectResult::HttpResponse(resp) if resp.status >= 400 => {
            err(ProviderError::from_http_status(resp.status))
        }
        SingleEffectResult::HttpResponse(resp) => {
            if let Some((owner, repo, kind, number)) = resource_info {
                let repo_path = RepoId::new(owner, repo);
                let cache_key = repo_path.cache_path(&format!("{}/{number}", kind.api_path()));
                let _ = with_state(|state| state.cache.set(cache_key, resp.body.clone()));
            }
            dir_entry(name)
        }
        SingleEffectResult::EffectError(_) => {
            ProviderResponse::Done(ActionResult::DirEntryOption(None))
        }
        _ => err(ProviderError::internal("unexpected result")),
    }
}

pub fn extract_str(json: &serde_json::Value, key: &str) -> String {
    json.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
        + "\n"
}

pub fn resume_comments(path: &str, result: &SingleEffectResult) -> ProviderResponse {
    let fs_path = FsPath::parse(path);

    // Helper to extract (owner, repo, number) from Comments or CommentFile variants.
    let comment_info = fs_path.as_ref().and_then(|p| match p {
        FsPath::Comments {
            owner,
            repo,
            number,
            ..
        }
        | FsPath::CommentFile {
            owner,
            repo,
            number,
            ..
        } => Some((*owner, *repo, *number)),
        _ => None,
    });

    if is_unauthorized(result) {
        enter_cache_only();
        if let Some((owner, repo, number)) = comment_info {
            let cache_key =
                RepoId::new(owner, repo).cache_path(&format!("issues/{number}/comments"));
            if let Ok(Some(data)) = get_cached(&cache_key) {
                return match &fs_path {
                    Some(FsPath::Comments { .. }) => list_cached_comments(&data),
                    Some(FsPath::CommentFile { idx, .. }) => serve_comment_file(&data, idx),
                    _ => ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                        entries: vec![],
                        exhaustive: false,
                    })),
                };
            }
        }
        if matches!(&fs_path, Some(FsPath::CommentFile { .. })) {
            return err(ProviderError::not_found("comment not found in cache"));
        }
        return ProviderResponse::Done(ActionResult::DirEntries(DirListing {
            entries: vec![],
            exhaustive: false,
        }));
    }
    let body = match super::extract_http_body(result) {
        Ok(b) => b,
        Err(e) => return e,
    };

    let Some((owner, repo, number)) = comment_info else {
        return err(ProviderError::internal("unexpected comments path"));
    };

    let cache_key = RepoId::new(owner, repo).cache_path(&format!("issues/{number}/comments"));
    let _ = with_state(|state| state.cache.set(cache_key, body.to_vec()));

    let json = match api::parse_json(body) {
        Ok(j) => j,
        Err(e) => return err(e),
    };

    match &fs_path {
        Some(FsPath::Comments { .. }) => {
            let Some(arr) = json.as_array() else {
                return err(ProviderError::internal(
                    "expected array in comments response",
                ));
            };
            let entries: Vec<DirEntry> = (1..=arr.len()).map(|i| mk_file(i.to_string())).collect();
            ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                entries,
                exhaustive: true,
            }))
        }
        Some(FsPath::CommentFile { idx, .. }) => serve_comment_file(body, idx),
        _ => err(ProviderError::internal("unexpected comments path")),
    }
}

pub fn serve_comment_file(body: &[u8], index_str: &str) -> ProviderResponse {
    let idx: usize = match index_str.parse::<usize>() {
        Ok(i) if i >= 1 => i,
        _ => return err(ProviderError::invalid_input("invalid comment index")),
    };

    let json = match api::parse_json(body) {
        Ok(j) => j,
        Err(e) => return err(e),
    };

    let Some(arr) = json.as_array() else {
        return err(ProviderError::internal("expected array in comments data"));
    };

    match arr.get(idx - 1) {
        Some(comment) => {
            let body_text = comment.get("body").and_then(|v| v.as_str()).unwrap_or("");
            let user = comment
                .get("user")
                .and_then(|u| u.get("login"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let content = format!("{user}:\n{body_text}\n");
            ProviderResponse::Done(ActionResult::FileContent(content.into_bytes()))
        }
        None => err(ProviderError::not_found("comment index out of range")),
    }
}

pub fn list_cached_comments(body: &[u8]) -> ProviderResponse {
    let json = match api::parse_json(body) {
        Ok(j) => j,
        Err(e) => return err(e),
    };

    let Some(arr) = json.as_array() else {
        return err(ProviderError::internal("expected array in comments data"));
    };

    let entries: Vec<DirEntry> = (1..=arr.len()).map(|i| mk_file(i.to_string())).collect();
    ProviderResponse::Done(ActionResult::DirEntries(DirListing {
        entries,
        exhaustive: true,
    }))
}
