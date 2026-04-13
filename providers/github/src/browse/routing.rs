//! Path routing using the `FsPath` parsed path type.
//!
//! Routes filesystem operations to the appropriate handlers based on
//! the parsed path structure.

use super::{cache_only, dir_entry, dispatch, err, file_entry, touch_repo};
use crate::api;
use crate::omnifs::provider::types::*;
use crate::path::{FsPath, Namespace, ResourceFile, ResourceKind, RunFile, StateFilter};
use crate::{CachedRepoListMode, Continuation, SingleEffect};

pub fn resolve_entry(id: u64, parent_path: &str, name: &str) -> ProviderResponse {
    let full_path = if parent_path.is_empty() {
        name.to_string()
    } else {
        format!("{parent_path}/{name}")
    };

    let Some(fs_path) = FsPath::parse(&full_path) else {
        return ProviderResponse::Done(ActionResult::DirEntryOption(None));
    };

    match fs_path {
        // Owner level: single segment, always valid
        FsPath::Owner { .. } => dir_entry(name),

        // Repo level: validate repo existence via API
        FsPath::Repo { owner, repo } => {
            if cache_only() {
                return dispatch(
                    id,
                    Continuation::ListingCachedRepos {
                        path: full_path.clone(),
                        mode: CachedRepoListMode::ValidateRepo,
                    },
                    SingleEffect::GitListCachedRepos(GitCacheListRequest {
                        prefix: Some(format!("github.com/{owner}/")),
                    }),
                );
            }
            let api_path = format!("/repos/{owner}/{repo}");
            dispatch(
                id,
                Continuation::ValidatingRepo {
                    path: full_path.clone(),
                },
                api::github_get(&api_path),
            )
        }

        // Namespace level: _repo, _issues, _prs, _actions
        FsPath::Namespace { ns, .. } => dir_entry(match ns {
            Namespace::Issues => "_issues",
            Namespace::Prs => "_prs",
            Namespace::Actions => "_actions",
            Namespace::Repo => "_repo",
        }),

        // State filter level: _open or _all
        FsPath::ResourceFilter { filter, .. } => dir_entry(match filter {
            StateFilter::Open => "_open",
            StateFilter::All => "_all",
        }),

        // Action runs directory
        FsPath::ActionRuns { .. } => dir_entry("runs"),

        // Resource (issue/PR) directory: validate via API if not cached
        FsPath::Resource {
            owner,
            repo,
            kind,
            number,
            ..
        } => {
            let api_resource = kind.api_path();
            let cache_key = format!("{owner}/{repo}/{api_resource}/{number}");
            let cached =
                super::with_state(|state| state.cache.get(&cache_key).is_some()).unwrap_or(false);
            if cached {
                return dir_entry(name);
            }
            if cache_only() {
                return ProviderResponse::Done(ActionResult::DirEntryOption(None));
            }
            // Fetch to validate existence and cache
            let api_path = format!("/repos/{owner}/{repo}/{api_resource}/{number}");
            dispatch(
                id,
                Continuation::ValidatingResource {
                    path: full_path.clone(),
                    name: name.to_string(),
                },
                api::github_get(&api_path),
            )
        }

        // Comments directory
        FsPath::Comments { .. } => dir_entry("comments"),

        // Resource files: title, body, state, user, diff
        FsPath::ResourceFile { file, kind, .. } => {
            let valid = matches!(
                file,
                ResourceFile::Title | ResourceFile::Body | ResourceFile::State | ResourceFile::User
            ) || (file == ResourceFile::Diff && kind == ResourceKind::Prs);
            if valid {
                file_entry(name)
            } else {
                ProviderResponse::Done(ActionResult::DirEntryOption(None))
            }
        }

        // Comment files
        FsPath::CommentFile { .. } => file_entry(name),

        // Action run directory
        FsPath::ActionRun { .. } => dir_entry(name),

        // Action run files: status, conclusion, log
        FsPath::ActionRunFile { file, .. } => {
            let valid = matches!(file, RunFile::Status | RunFile::Conclusion | RunFile::Log);
            if valid {
                file_entry(name)
            } else {
                ProviderResponse::Done(ActionResult::DirEntryOption(None))
            }
        }

        // Repo tree: disown to FUSE passthrough
        FsPath::RepoTree { owner, repo, .. } => {
            let clone_url = format!("git@github.com:{owner}/{repo}.git");
            let cache_key = format!("github.com/{owner}/{repo}");
            dispatch(
                id,
                Continuation::DisowningRepo,
                SingleEffect::GitOpenRepo(GitOpenRequest { clone_url, cache_key }),
            )
        }

        // Root level not valid here
        FsPath::Root => ProviderResponse::Done(ActionResult::DirEntryOption(None)),
    }
}

pub fn list_entries(id: u64, path: &str) -> ProviderResponse {
    let Some(fs_path) = FsPath::parse(path) else {
        return err("invalid path");
    };

    match fs_path {
        // Root level: list owners from cache
        FsPath::Root => dispatch(
            id,
            Continuation::ListingCachedRepos {
                path: path.to_string(),
                mode: CachedRepoListMode::Root,
            },
            SingleEffect::GitListCachedRepos(GitCacheListRequest {
                prefix: Some("github.com/".to_string()),
            }),
        ),

        // Owner level: list repos
        FsPath::Owner { owner } => {
            if cache_only() {
                return dispatch(
                    id,
                    Continuation::ListingCachedRepos {
                        path: path.to_string(),
                        mode: CachedRepoListMode::Owner,
                    },
                    SingleEffect::GitListCachedRepos(GitCacheListRequest {
                        prefix: Some(format!("github.com/{owner}/")),
                    }),
                );
            }
            // Check negative owner cache (TTL-based)
            let is_negative = super::with_state(|state| {
                if let Some(&cached_tick) = state.negative_owners.get(owner) {
                    let now = state.cache.current_tick();
                    now.saturating_sub(cached_tick) < super::NEGATIVE_OWNER_TTL
                } else {
                    false
                }
            })
            .unwrap_or(false);
            if is_negative {
                return ProviderResponse::Done(ActionResult::DirEntries(vec![]));
            }
            // Return from repo list cache if fresh.
            let cached = super::with_state(|state| {
                if let Some((tick, repos)) = state.owner_repos_cache.get(owner) {
                    let now = state.cache.current_tick();
                    if now.saturating_sub(*tick) < crate::OWNER_REPOS_CACHE_TTL {
                        return Some(repos.clone());
                    }
                }
                None
            })
            .unwrap_or(None);
            if let Some(repos) = cached {
                let entries = repos
                    .into_iter()
                    .map(|name| DirEntry {
                        name,
                        kind: EntryKind::Directory,
                        size: None,
                        projected_files: None,
                    })
                    .collect();
                return ProviderResponse::Done(ActionResult::DirEntries(entries));
            }
            // Fetch owner profile to determine kind and repo count.
            let known_kind = super::with_state(|state| {
                state.owner_kinds.get(owner).copied()
            })
            .unwrap_or(None);
            let api_path = match known_kind {
                Some(crate::OwnerKind::Org) => format!("/orgs/{owner}"),
                _ => format!("/users/{owner}"),
            };
            dispatch(
                id,
                Continuation::FetchingOwnerProfile {
                    path: path.to_string(),
                    is_org_fallback: known_kind == Some(crate::OwnerKind::Org),
                },
                api::github_get(&api_path),
            )
        }

        // Repo level: fixed namespace dirs
        FsPath::Repo { owner, repo } => {
            touch_repo(owner, repo);
            ProviderResponse::Done(ActionResult::DirEntries(vec![
                DirEntry {
                    name: "_repo".to_string(),
                    kind: EntryKind::Directory,
                    size: None,
                    projected_files: None,
                },
                DirEntry {
                    name: "_issues".to_string(),
                    kind: EntryKind::Directory,
                    size: None,
                    projected_files: None,
                },
                DirEntry {
                    name: "_prs".to_string(),
                    kind: EntryKind::Directory,
                    size: None,
                    projected_files: None,
                },
                DirEntry {
                    name: "_actions".to_string(),
                    kind: EntryKind::Directory,
                    size: None,
                    projected_files: None,
                },
            ]))
        }

        // Namespace level: list appropriate subdirs
        FsPath::Namespace { owner, repo, ns } => {
            touch_repo(owner, repo);
            match ns {
                Namespace::Issues | Namespace::Prs => {
                    ProviderResponse::Done(ActionResult::DirEntries(vec![
                        DirEntry {
                            name: "_open".to_string(),
                            kind: EntryKind::Directory,
                            size: None,
                            projected_files: None,
                        },
                        DirEntry {
                            name: "_all".to_string(),
                            kind: EntryKind::Directory,
                            size: None,
                            projected_files: None,
                        },
                    ]))
                }
                Namespace::Actions => {
                    ProviderResponse::Done(ActionResult::DirEntries(vec![DirEntry {
                        name: "runs".to_string(),
                        kind: EntryKind::Directory,
                        size: None,
                        projected_files: None,
                    }]))
                }
                Namespace::Repo => {
                    let clone_url = format!("git@github.com:{owner}/{repo}.git");
                    let cache_key = format!("github.com/{owner}/{repo}");
                    dispatch(
                        id,
                        Continuation::DisowningRepo,
                        SingleEffect::GitOpenRepo(GitOpenRequest { clone_url, cache_key }),
                    )
                }
            }
        }

        // Resource filter level: list issues/PRs
        FsPath::ResourceFilter {
            owner,
            repo,
            kind,
            filter,
        } => {
            touch_repo(owner, repo);
            if cache_only() {
                return super::finalize_cached_resource_list(owner, repo, kind, filter);
            }
            let resource_kind = kind.search_qualifier();
            let state_clause = match filter {
                StateFilter::Open => "+state:open",
                StateFilter::All => "",
            };
            let query = format!("repo:{owner}/{repo}+is:{resource_kind}{state_clause}");
            let api_path = format!("/search/issues?q={query}&sort=created&order=desc&per_page=100");
            dispatch(
                id,
                Continuation::FetchingFirstPage {
                    path: path.to_string(),
                    is_org_fallback: false,
                },
                api::github_get(&api_path),
            )
        }

        // Resource level: list files under issue/PR
        FsPath::Resource {
            owner, repo, kind, ..
        } => {
            touch_repo(owner, repo);
            let mut files = vec![
                DirEntry {
                    name: "title".to_string(),
                    kind: EntryKind::File,
                    size: Some(4096),
                    projected_files: None,
                },
                DirEntry {
                    name: "body".to_string(),
                    kind: EntryKind::File,
                    size: Some(4096),
                    projected_files: None,
                },
                DirEntry {
                    name: "state".to_string(),
                    kind: EntryKind::File,
                    size: Some(4096),
                    projected_files: None,
                },
                DirEntry {
                    name: "user".to_string(),
                    kind: EntryKind::File,
                    size: Some(4096),
                    projected_files: None,
                },
                DirEntry {
                    name: "comments".to_string(),
                    kind: EntryKind::Directory,
                    size: None,
                    projected_files: None,
                },
            ];
            if kind == ResourceKind::Prs {
                files.push(DirEntry {
                    name: "diff".to_string(),
                    kind: EntryKind::File,
                    size: Some(4096),
                    projected_files: None,
                });
            }
            ProviderResponse::Done(ActionResult::DirEntries(files))
        }

        // Comments level: list or fetch comments
        FsPath::Comments {
            owner,
            repo,
            number,
            ..
        } => {
            touch_repo(owner, repo);
            let cache_key = format!("{owner}/{repo}/issues/{number}/comments");
            if let Ok(Some(data)) =
                super::with_state(|state| state.cache.get(&cache_key).map(<[u8]>::to_vec))
            {
                return super::list_cached_comments(&data);
            }
            if cache_only() {
                return ProviderResponse::Done(ActionResult::DirEntries(vec![]));
            }
            let api_path = format!("/repos/{owner}/{repo}/issues/{number}/comments?per_page=100");
            dispatch(
                id,
                Continuation::FetchingComments {
                    path: path.to_string(),
                },
                api::github_get(&api_path),
            )
        }

        // Repo tree: disown to FUSE passthrough
        FsPath::RepoTree {
            owner,
            repo,
            ..
        } => {
            touch_repo(owner, repo);
            let clone_url = format!("git@github.com:{owner}/{repo}.git");
            let cache_key = format!("github.com/{owner}/{repo}");
            dispatch(
                id,
                Continuation::DisowningRepo,
                SingleEffect::GitOpenRepo(GitOpenRequest { clone_url, cache_key }),
            )
        }

        // Action runs level: list runs
        FsPath::ActionRuns { owner, repo } => {
            touch_repo(owner, repo);
            if cache_only() {
                return super::finalize_cached_runs_list(owner, repo);
            }
            let api_path = format!("/repos/{owner}/{repo}/actions/runs?per_page=30");
            dispatch(
                id,
                Continuation::FetchingFirstPage {
                    path: path.to_string(),
                    is_org_fallback: false,
                },
                api::github_get(&api_path),
            )
        }

        // Action run level: list run files
        FsPath::ActionRun { .. } => ProviderResponse::Done(ActionResult::DirEntries(vec![
            DirEntry {
                name: "status".to_string(),
                kind: EntryKind::File,
                size: Some(4096),
                projected_files: None,
            },
            DirEntry {
                name: "conclusion".to_string(),
                kind: EntryKind::File,
                size: Some(4096),
                projected_files: None,
            },
            DirEntry {
                name: "log".to_string(),
                kind: EntryKind::File,
                size: Some(4096),
                projected_files: None,
            },
        ])),

        // All other paths: not listable
        _ => err("not found"),
    }
}

pub fn read_file(id: u64, path: &str) -> ProviderResponse {
    let Some(fs_path) = FsPath::parse(path) else {
        return err("invalid path");
    };

    // Touch repo if owner/repo is present
    if let (Some(owner), Some(repo)) = (fs_path.owner(), fs_path.repo()) {
        touch_repo(owner, repo);
    }

    match fs_path {
        // Comment file reads
        FsPath::CommentFile {
            owner,
            repo,
            number,
            idx,
            ..
        } => {
            let cache_key = format!("{owner}/{repo}/issues/{number}/comments");
            if let Ok(Some(data)) =
                super::with_state(|state| state.cache.get(&cache_key).map(<[u8]>::to_vec))
            {
                return super::serve_comment_file(&data, idx);
            }
            if cache_only() {
                return err("not found in cache");
            }
            // Fetch comments and cache them
            let api_path = format!("/repos/{owner}/{repo}/issues/{number}/comments?per_page=100");
            dispatch(
                id,
                Continuation::FetchingComments {
                    path: path.to_string(),
                },
                api::github_get(&api_path),
            )
        }

        // Issue/PR file reads
        FsPath::ResourceFile {
            owner,
            repo,
            kind,
            number,
            file,
            ..
        } => {
            let api_resource = kind.api_path();

            // Diff requires separate fetch with different Accept header
            if file == ResourceFile::Diff {
                let diff_cache_key = format!("{owner}/{repo}/pulls/{number}/diff");
                if let Ok(Some(data)) =
                    super::with_state(|state| state.cache.get(&diff_cache_key).map(<[u8]>::to_vec))
                {
                    return ProviderResponse::Done(ActionResult::FileContent(data));
                }
                if cache_only() {
                    return err("not found in cache");
                }
                let url = format!("https://api.github.com/repos/{owner}/{repo}/pulls/{number}");
                return dispatch(
                    id,
                    Continuation::FetchingDiff {
                        path: path.to_string(),
                    },
                    SingleEffect::Fetch(HttpRequest {
                        method: "GET".to_string(),
                        url,
                        headers: vec![Header {
                            name: "Accept".to_string(),
                            value: "application/vnd.github.diff".to_string(),
                        }],
                        body: None,
                    }),
                );
            }

            let cache_key = format!("{owner}/{repo}/{api_resource}/{number}");

            // Check cache
            if let Ok(Some(data)) =
                super::with_state(|state| state.cache.get(&cache_key).map(<[u8]>::to_vec))
            {
                return super::serve_resource_file(&data, file);
            }
            if cache_only() {
                return err("not found in cache");
            }

            let api_path = format!("/repos/{owner}/{repo}/{api_resource}/{number}");
            dispatch(
                id,
                Continuation::FetchingResource {
                    path: path.to_string(),
                },
                api::github_get(&api_path),
            )
        }

        // Action run file reads
        FsPath::ActionRunFile {
            owner,
            repo,
            run_id,
            file,
        } => {
            // Log file requires a separate endpoint and zip extraction.
            if file == RunFile::Log {
                let log_cache_key = format!("{owner}/{repo}/actions/runs/{run_id}/log");
                if let Ok(Some(data)) =
                    super::with_state(|state| state.cache.get(&log_cache_key).map(<[u8]>::to_vec))
                {
                    return ProviderResponse::Done(ActionResult::FileContent(data));
                }
                if cache_only() {
                    return err("not found in cache");
                }
                let api_path = format!("/repos/{owner}/{repo}/actions/runs/{run_id}/logs");
                return dispatch(
                    id,
                    Continuation::FetchingRunLog {
                        path: path.to_string(),
                    },
                    api::github_get(&api_path),
                );
            }

            let cache_key = format!("{owner}/{repo}/actions/runs/{run_id}");
            if let Ok(Some(data)) =
                super::with_state(|state| state.cache.get(&cache_key).map(<[u8]>::to_vec))
            {
                return super::serve_run_file(&data, file);
            }
            if cache_only() {
                return err("not found in cache");
            }

            let api_path = format!("/repos/{owner}/{repo}/actions/runs/{run_id}");
            dispatch(
                id,
                Continuation::FetchingResource {
                    path: path.to_string(),
                },
                api::github_get(&api_path),
            )
        }

        // Repo tree reads: disown to FUSE passthrough
        FsPath::RepoTree {
            owner,
            repo,
            ..
        } => {
            let clone_url = format!("git@github.com:{owner}/{repo}.git");
            let cache_key = format!("github.com/{owner}/{repo}");
            dispatch(
                id,
                Continuation::DisowningRepo,
                SingleEffect::GitOpenRepo(GitOpenRequest { clone_url, cache_key }),
            )
        }

        // All other paths: not readable as files
        _ => err("not found"),
    }
}
