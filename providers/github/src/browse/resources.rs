//! Resource listing and search continuations.
//!
//! Handles listing cached repos, fetching search results for issues/PRs,
//! and paginating through GitHub API responses.

use super::{dispatch, enter_cache_only, err, is_unauthorized, with_state};
use crate::api;
use crate::omnifs::provider::types::*;
use crate::path::{FsPath, ResourceKind, StateFilter};
use crate::{Continuation, SingleEffect};
use hashbrown::HashSet;

pub fn resume_cached_repos(
    path: &str,
    mode: &super::CachedRepoListMode,
    result: &SingleEffectResult,
) -> ProviderResponse {
    let repos = match result {
        SingleEffectResult::GitCachedRepos(repos) => repos,
        SingleEffectResult::EffectError(e) => {
            return err(&format!("git cache list failed: {}", e.message));
        }
        _ => return err("unexpected cached repo result"),
    };

    match mode {
        super::CachedRepoListMode::Root => {
            let mut owners = HashSet::new();
            for repo in repos {
                // cache_key format: "github.com/owner/repo"
                if let Some(owner) = repo.cache_key.strip_prefix("github.com/").and_then(|rest| rest.split('/').next()) {
                    owners.insert(owner.to_string());
                }
            }
            let mut entries: Vec<DirEntry> = owners
                .into_iter()
                .map(|name| DirEntry {
                    name,
                    kind: EntryKind::Directory,
                    size: None,
                    projected_files: None,
                })
                .collect();
            entries.sort_by(|a, b| a.name.cmp(&b.name));
            ProviderResponse::Done(ActionResult::DirEntries(entries))
        }
        super::CachedRepoListMode::Owner => {
            let mut entries: Vec<DirEntry> = repos
                .iter()
                .filter_map(|repo| {
                    // cache_key format: "github.com/owner/repo"
                    let rest = repo.cache_key.strip_prefix("github.com/")?;
                    let repo_name = rest.split('/').nth(1)?;
                    Some(DirEntry {
                        name: repo_name.to_string(),
                        kind: EntryKind::Directory,
                        size: None,
                        projected_files: None,
                    })
                })
                .collect();
            entries.sort_by(|a, b| a.name.cmp(&b.name));
            ProviderResponse::Done(ActionResult::DirEntries(entries))
        }
        super::CachedRepoListMode::ValidateRepo => {
            let Some(FsPath::Repo {
                repo: repo_name, ..
            }) = FsPath::parse(path)
            else {
                return ProviderResponse::Done(ActionResult::DirEntryOption(None));
            };
            // Check if any cache key ends with /repo_name
            if repos.iter().any(|repo| {
                repo.cache_key
                    .strip_prefix("github.com/")
                    .and_then(|rest| rest.split('/').nth(1))
                    .is_some_and(|name| name == repo_name)
            }) {
                super::dir_entry(repo_name)
            } else {
                ProviderResponse::Done(ActionResult::DirEntryOption(None))
            }
        }
    }
}

/// Handle owner profile response. Extracts owner kind and `public_repos` count,
/// then dispatches all repo listing pages as a parallel Batch.
pub fn resume_owner_profile(
    id: u64,
    path: &str,
    is_org_fallback: bool,
    result: &SingleEffectResult,
) -> ProviderResponse {
    let Some(FsPath::Owner { owner }) = FsPath::parse(path) else {
        return err("expected owner path");
    };

    let status = match result {
        SingleEffectResult::HttpResponse(resp) => resp.status,
        _ => 0,
    };
    if status == 404 {
        if is_org_fallback {
            let _ = with_state(|state| {
                let tick = state.cache.current_tick();
                state.negative_owners.insert(owner.to_string(), tick);
            });
            return ProviderResponse::Done(ActionResult::DirEntries(vec![]));
        }
        return dispatch(
            id,
            Continuation::FetchingOwnerProfile {
                path: path.to_string(),
                is_org_fallback: true,
            },
            crate::api::github_get(&format!("/orgs/{owner}")),
        );
    }

    if is_unauthorized(result) {
        enter_cache_only();
        return dispatch(
            id,
            Continuation::ListingCachedRepos {
                path: path.to_string(),
                mode: super::CachedRepoListMode::Owner,
            },
            SingleEffect::GitListCachedRepos(GitCacheListRequest {
                prefix: Some(format!("github.com/{owner}/")),
            }),
        );
    }

    let body = match super::extract_http_body(result) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let json = match crate::api::parse_json(body) {
        Ok(j) => j,
        Err(e) => return err(&e),
    };

    let owner_kind = match json.get("type").and_then(|v| v.as_str()) {
        Some("Organization") => crate::OwnerKind::Org,
        _ => crate::OwnerKind::User,
    };
    let _ = with_state(|state| {
        state.owner_kinds.insert(owner.to_string(), owner_kind);
        state.negative_owners.remove(owner);
    });

    let public_repos = json
        .get("public_repos")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    let per_page = 100u64;
    #[allow(clippy::cast_possible_truncation)]
    let page_count = public_repos.div_ceil(per_page).clamp(1, 50) as u32;

    let repos_path = match owner_kind {
        crate::OwnerKind::Org => format!("/orgs/{owner}/repos?per_page=100&sort=updated"),
        crate::OwnerKind::User => format!("/users/{owner}/repos?per_page=100&sort=updated"),
    };

    if page_count <= 1 {
        return dispatch(
            id,
            Continuation::FetchingRepoPages {
                path: path.to_string(),
            },
            crate::api::github_get(&repos_path),
        );
    }

    let fetches: Vec<SingleEffect> = (1..=page_count)
        .map(|page| crate::api::github_get(&format!("{repos_path}&page={page}")))
        .collect();

    match with_state(|state| {
        state.pending.insert(
            id,
            Continuation::FetchingRepoPages {
                path: path.to_string(),
            },
        );
    }) {
        Ok(()) => ProviderResponse::Batch(fetches),
        Err(e) => err(&e),
    }
}

/// Handle repo listing pages (single or batch). Merge results, cache, return entries.
pub fn resume_repo_pages(
    _id: u64,
    path: &str,
    effect_outcome: &EffectResult,
) -> ProviderResponse {
    let Some(FsPath::Owner { owner }) = FsPath::parse(path) else {
        return err("expected owner path");
    };

    let results: Vec<&SingleEffectResult> = match effect_outcome {
        EffectResult::Single(r) => vec![r],
        EffectResult::Batch(results) => results.iter().collect(),
    };

    let mut repos = Vec::new();
    for result in results {
        if let SingleEffectResult::HttpResponse(resp) = result
            && resp.status < 400
        {
            super::check_rate_limit(resp);
            if let Ok(json) = crate::api::parse_json(&resp.body)
                && let Some(arr) = json.as_array()
            {
                for item in arr {
                    if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                        repos.push(name.to_string());
                    }
                }
            }
        }
    }

    repos.sort();
    repos.dedup();

    let _ = with_state(|state| {
        let tick = state.cache.current_tick();
        state
            .owner_repos_cache
            .insert(owner.to_string(), (tick, repos.clone()));
    });

    let entries = repos
        .into_iter()
        .map(|name| DirEntry {
            name,
            kind: EntryKind::Directory,
            size: None,
            projected_files: None,
        })
        .collect();
    ProviderResponse::Done(ActionResult::DirEntries(entries))
}

/// Handle page 1 of a list response. For search API (issues/PRs), if
/// `total_count` > 100, dispatch remaining pages as a Batch for parallel fetch.
pub fn resume_list_first_page(
    id: u64,
    path: &str,
    is_org_fallback: bool,
    result: &SingleEffectResult,
) -> ProviderResponse {
    let fs_path = FsPath::parse(path);

    if is_unauthorized(result) {
        enter_cache_only();
        if let Some(ref p) = fs_path {
            match p {
                FsPath::Owner { owner } => {
                    return dispatch(
                        id,
                        Continuation::ListingCachedRepos {
                            path: path.to_string(),
                            mode: super::CachedRepoListMode::Owner,
                        },
                        SingleEffect::GitListCachedRepos(GitCacheListRequest {
                            prefix: Some(format!("github.com/{}/", *owner)),
                        }),
                    );
                }
                FsPath::ResourceFilter {
                    owner,
                    repo,
                    kind,
                    filter,
                } => {
                    return super::finalize_cached_resource_list(owner, repo, *kind, *filter);
                }
                FsPath::ActionRuns { owner, repo } => {
                    return super::finalize_cached_runs_list(owner, repo);
                }
                _ => {}
            }
        }
    }

    // Owner-level repo listing: handle user/org fallback on 404.
    if let Some(FsPath::Owner { owner }) = &fs_path {
        let status = match result {
            SingleEffectResult::HttpResponse(resp) => resp.status,
            _ => 0,
        };
        if status == 404 {
            if is_org_fallback {
                let _ = with_state(|state| {
                    let tick = state.cache.current_tick();
                    state.negative_owners.insert((*owner).to_string(), tick);
                });
                return ProviderResponse::Done(ActionResult::DirEntries(vec![]));
            }
            let api_path = format!("/orgs/{owner}/repos?per_page=100&sort=updated");
            return dispatch(
                id,
                Continuation::FetchingFirstPage {
                    path: path.to_string(),
                    is_org_fallback: true,
                },
                api::github_get(&api_path),
            );
        }
    }

    let body = match super::extract_http_body(result) {
        Ok(b) => b,
        Err(e) => return e,
    };

    let json = match api::parse_json(body) {
        Ok(j) => j,
        Err(e) => return err(&e),
    };

    match &fs_path {
        Some(FsPath::Owner { .. }) => {
            let Some(arr) = json.as_array() else {
                return err("expected array in repo listing response");
            };
            let entries = arr
                .iter()
                .filter_map(|item| {
                    let name = item.get("name")?.as_str()?;
                    Some(DirEntry {
                        name: name.to_string(),
                        kind: EntryKind::Directory,
                        size: None,
                        projected_files: None,
                    })
                })
                .collect();
            ProviderResponse::Done(ActionResult::DirEntries(entries))
        }
        Some(FsPath::ResourceFilter {
            owner,
            repo,
            kind,
            filter,
        }) => {
            let total_count = json
                .get("total_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let Some(items) = json.get("items").and_then(|v| v.as_array()) else {
                return err("expected 'items' array in search response");
            };
            let first_page_items: Vec<serde_json::Value> = items.clone();

            let per_page = 100u64;
            let fetchable = total_count.min(1000);
            let page_count = fetchable.div_ceil(per_page);

            if page_count <= 1 {
                return super::finalize_search_results(path, &first_page_items);
            }

            let resource_kind = kind.search_qualifier();
            let state_clause = match filter {
                StateFilter::Open => "+state:open",
                StateFilter::All => "",
            };
            let query = format!("repo:{owner}/{repo}+is:{resource_kind}{state_clause}");
            let base = format!("/search/issues?q={query}&sort=created&order=desc&per_page=100");

            let remaining_fetches: Vec<SingleEffect> = (2..=page_count as u32)
                .map(|page| api::github_get(&format!("{base}&page={page}")))
                .collect();

            match with_state(|state| {
                state.pending.insert(
                    id,
                    Continuation::FetchingRemainingPages {
                        path: path.to_string(),
                        first_page_items,
                    },
                );
            }) {
                Ok(()) => ProviderResponse::Batch(remaining_fetches),
                Err(e) => err(&e),
            }
        }
        Some(FsPath::ActionRuns { owner, repo }) => {
            let Some(runs) = json.get("workflow_runs").and_then(|v| v.as_array()) else {
                return err("expected 'workflow_runs' array in runs response");
            };
            let entries = runs
                .iter()
                .filter_map(|item| {
                    let run_id = item.get("id")?.as_u64()?;
                    let cache_key = format!("{owner}/{repo}/actions/runs/{run_id}");
                    let item_bytes = serde_json::to_vec(item).ok()?;
                    let _ = with_state(|state| state.cache.set(cache_key, item_bytes));
                    Some(DirEntry {
                        name: run_id.to_string(),
                        kind: EntryKind::Directory,
                        size: None,
                        projected_files: None,
                    })
                })
                .collect();
            ProviderResponse::Done(ActionResult::DirEntries(entries))
        }
        _ => err("unexpected list path"),
    }
}

/// Handle remaining pages (from a Batch response). Merge with first page items.
pub fn resume_list_remaining(
    _id: u64,
    path: &str,
    first_page_items: Vec<serde_json::Value>,
    effect_outcome: &EffectResult,
) -> ProviderResponse {
    let batch_results = match effect_outcome {
        EffectResult::Batch(results) => results,
        EffectResult::Single(_) => return err("expected batch result for remaining pages"),
    };

    let mut all_items = first_page_items;

    for result in batch_results {
        if let SingleEffectResult::HttpResponse(resp) = result
            && resp.status < 400
            && let Ok(json) = api::parse_json(&resp.body)
            && let Some(items) = json.get("items").and_then(|v| v.as_array())
        {
            all_items.extend(items.iter().cloned());
        }
    }

    super::finalize_search_results(path, &all_items)
}

fn build_projected_files(item: &serde_json::Value) -> Vec<ProjectedFile> {
    let title = item
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
        + "\n";
    let body = item
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
        + "\n";
    let state = item
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
        + "\n";
    let user = item
        .get("user")
        .and_then(|u| u.get("login"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
        + "\n";

    vec![
        ProjectedFile { name: "title".to_string(), content: title.into_bytes() },
        ProjectedFile { name: "body".to_string(), content: body.into_bytes() },
        ProjectedFile { name: "state".to_string(), content: state.into_bytes() },
        ProjectedFile { name: "user".to_string(), content: user.into_bytes() },
    ]
}

/// Convert a list of search result items into `DirEntries`, caching each resource.
pub fn finalize_search_results(path: &str, items: &[serde_json::Value]) -> ProviderResponse {
    let Some(FsPath::ResourceFilter {
        owner, repo, kind, ..
    }) = FsPath::parse(path)
    else {
        return err("invalid resource filter path");
    };
    let api_resource = kind.api_path();

    let entries = items
        .iter()
        .filter_map(|item| {
            let number = item.get("number")?.as_u64()?;
            let cache_key = format!("{owner}/{repo}/{api_resource}/{number}");
            let item_bytes = serde_json::to_vec(item).ok()?;
            let _ = with_state(|state| state.cache.set(cache_key, item_bytes));
            // Build projected files from the search result JSON.
            let projected = build_projected_files(item);
            Some(DirEntry {
                name: number.to_string(),
                kind: EntryKind::Directory,
                size: None,
                projected_files: Some(projected),
            })
        })
        .collect();
    ProviderResponse::Done(ActionResult::DirEntries(entries))
}

pub fn finalize_cached_resource_list(
    owner: &str,
    repo: &str,
    kind: ResourceKind,
    filter: StateFilter,
) -> ProviderResponse {
    let api_resource = kind.api_path();
    let prefix = format!("{owner}/{repo}/{api_resource}/");
    let keys = with_state(|state| state.cache.keys_with_prefix(&prefix)).unwrap_or_default();
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for key in keys {
        let Some(number) = key.strip_prefix(&prefix) else {
            continue;
        };
        if number.contains('/') || !seen.insert(number.to_string()) {
            continue;
        }
        if filter == StateFilter::Open
            && !with_state(|state| {
                state
                    .cache
                    .get(&key)
                    .and_then(|data| api::parse_json(data).ok())
                    .and_then(|json| {
                        json.get("state")
                            .and_then(|state| state.as_str())
                            .map(|state| state == "open")
                    })
                    .unwrap_or(false)
            })
            .unwrap_or(false)
        {
            continue;
        }
        entries.push(DirEntry {
            name: number.to_string(),
            kind: EntryKind::Directory,
            size: None,
            projected_files: None,
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    ProviderResponse::Done(ActionResult::DirEntries(entries))
}

pub fn finalize_cached_runs_list(owner: &str, repo: &str) -> ProviderResponse {
    let prefix = format!("{owner}/{repo}/actions/runs/");
    let keys = with_state(|state| state.cache.keys_with_prefix(&prefix)).unwrap_or_default();
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for key in keys {
        let Some(run_id) = key.strip_prefix(&prefix) else {
            continue;
        };
        if run_id.contains('/') || !seen.insert(run_id.to_string()) {
            continue;
        }
        entries.push(DirEntry {
            name: run_id.to_string(),
            kind: EntryKind::Directory,
            size: None,
            projected_files: None,
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    ProviderResponse::Done(ActionResult::DirEntries(entries))
}
