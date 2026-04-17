//! github-provider: GitHub virtual filesystem provider for omnifs.
//!
//! Exposes GitHub resources (issues, PRs, actions, repository contents)
//! as a virtual filesystem using the omnifs provider WIT interface.

use omnifs_sdk::prelude::*;

mod api;
mod browse;
pub(crate) mod path;
pub(crate) mod types;

use crate::browse::dispatch_or_err;
use types::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnerKind {
    User,
    Org,
}

#[omnifs_sdk::config]
pub struct Config {}

pub struct State {
    cache: Cache,
    negative_owners: hashbrown::HashMap<String, u64>,
    owner_kinds: hashbrown::HashMap<String, OwnerKind>,
    owner_repos_cache: hashbrown::HashMap<String, (u64, Vec<String>)>,
    rate_limit_remaining: Option<u32>,
    cache_only: bool,
    active_repos: hashbrown::HashMap<String, u64>,
    event_etags: hashbrown::HashMap<String, String>,
    pending_host_invalidations: Vec<String>,
}

const OWNER_REPOS_CACHE_TTL: u64 = 120;

pub enum Continuation {
    ListingCachedRepos {
        path: String,
        mode: CachedRepoListMode,
    },
    FetchingFirstPage {
        path: String,
        is_org_fallback: bool,
    },
    FetchingOwnerProfile {
        path: String,
        is_org_fallback: bool,
    },
    FetchingRepoPages {
        path: String,
    },
    FetchingRemainingPages {
        path: String,
        first_page_items: Vec<serde_json::Value>,
    },
    FetchingResource {
        path: String,
    },
    ValidatingRepo {
        path: String,
    },
    ValidatingResource {
        path: String,
        name: String,
    },
    FetchingComments {
        path: String,
    },
    DisowningRepo,
    FetchingDiff {
        path: String,
    },
    FetchingRunLog {
        path: String,
    },
    FetchingEvents {
        repos: Vec<String>,
        invalidation_count: usize,
    },
}

pub enum CachedRepoListMode {
    Root,
    Owner,
    ValidateRepo,
}

#[omnifs_sdk::provider]
impl GithubProvider {
    fn init(_config: Config) -> (State, ProviderInfo) {
        let mut state = State {
            cache: Cache::new(128),
            negative_owners: hashbrown::HashMap::new(),
            owner_kinds: hashbrown::HashMap::new(),
            owner_repos_cache: hashbrown::HashMap::new(),
            rate_limit_remaining: None,
            cache_only: false,
            active_repos: hashbrown::HashMap::new(),
            event_etags: hashbrown::HashMap::new(),
            pending_host_invalidations: Vec::new(),
        };
        state.cache.advance_tick();

        let info = ProviderInfo {
            name: "github-provider".to_string(),
            version: "0.1.0".to_string(),
            description: "GitHub API provider for omnifs".to_string(),
        };
        (state, info)
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["api.github.com".to_string()],
            auth_types: vec!["bearer-token".to_string()],
            max_memory_mb: 128,
            needs_git: true,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 60,
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    fn resume(id: u64, cont: Continuation, outcome: EffectResult) -> ProviderResponse {
        browse::resume(id, cont, outcome)
    }

    #[allow(clippy::needless_pass_by_value)]
    fn on_event(id: u64, event: ProviderEvent) -> ProviderResponse {
        match event {
            ProviderEvent::TimerTick => browse::timer_tick(id),
            _ => ProviderResponse::Done(ActionResult::Ok),
        }
    }

    // --- Route handlers (source order = priority) ---

    #[route("/")]
    fn root(op: Op) -> Option<ProviderResponse> {
        match op {
            Op::List(id) => Some(dispatch_or_err(
                id,
                Continuation::ListingCachedRepos {
                    path: String::new(),
                    mode: CachedRepoListMode::Root,
                },
                SingleEffect::GitListCachedRepos(GitCacheListRequest {
                    prefix: Some("github.com/".to_string()),
                }),
            )),
            Op::Lookup(_) | Op::Read(_) => None,
        }
    }

    #[route("/{owner}")]
    fn owner_handler(op: Op, owner: Owner) -> Option<ProviderResponse> {
        let owner = owner.as_str();
        dir_only_no_read(op, owner, |id| Self::list_owner(id, owner))
    }

    #[route("/{owner}/{repo}")]
    fn repo_handler(op: Op, owner: Owner, repo: Repo) -> Option<ProviderResponse> {
        let owner = owner.as_str();
        let repo = repo.as_str();
        match op {
            Op::Lookup(id) => Some(Self::lookup_repo(id, owner, repo)),
            Op::List(_) => {
                browse::touch_repo(owner, repo);
                Some(ProviderResponse::Done(ActionResult::DirEntries(
                    DirListing {
                        entries: vec![
                            mk_dir(Namespace::Repo.as_ref()),
                            mk_dir(Namespace::Issues.as_ref()),
                            mk_dir(Namespace::Prs.as_ref()),
                            mk_dir(Namespace::Actions.as_ref()),
                        ],
                        exhaustive: true,
                    },
                )))
            }
            Op::Read(_) => None,
        }
    }

    #[route("/{owner}/{repo}/_issues")]
    fn ns_issues(op: Op, owner: Owner, repo: Repo) -> Option<ProviderResponse> {
        let owner = owner.as_str();
        let repo = repo.as_str();
        Self::namespace_handler(op, owner, repo, Namespace::Issues)
    }

    #[route("/{owner}/{repo}/_prs")]
    fn ns_prs(op: Op, owner: Owner, repo: Repo) -> Option<ProviderResponse> {
        let owner = owner.as_str();
        let repo = repo.as_str();
        Self::namespace_handler(op, owner, repo, Namespace::Prs)
    }

    #[route("/{owner}/{repo}/_actions")]
    fn ns_actions(op: Op, owner: Owner, repo: Repo) -> Option<ProviderResponse> {
        let owner = owner.as_str();
        let repo = repo.as_str();
        Self::namespace_handler(op, owner, repo, Namespace::Actions)
    }

    #[route("/{owner}/{repo}/_repo")]
    fn ns_repo(op: Op, owner: Owner, repo: Repo) -> Option<ProviderResponse> {
        let owner = owner.as_str();
        let repo = repo.as_str();
        Self::namespace_handler(op, owner, repo, Namespace::Repo)
    }

    #[route("/{owner}/{repo}/_repo/{*tree_path}")]
    fn repo_tree(
        op: Op,
        owner: Owner,
        repo: Repo,
        tree_path: TreePath,
    ) -> Option<ProviderResponse> {
        let owner = owner.as_str();
        let repo = repo.as_str();
        let _tree_path = tree_path.as_str();
        browse::touch_repo(owner, repo);
        let repo_path = RepoPath::new(owner, repo);
        let clone_url = repo_path.clone_url();
        let cache_key = repo_path.github_cache_key();
        Some(dispatch_or_err(
            op.id(),
            Continuation::DisowningRepo,
            SingleEffect::GitOpenRepo(GitOpenRequest {
                clone_url,
                cache_key,
            }),
        ))
    }

    #[route("/{owner}/{repo}/_actions/runs")]
    fn action_runs(op: Op, owner: Owner, repo: Repo) -> Option<ProviderResponse> {
        let owner = owner.as_str();
        let repo = repo.as_str();
        file_only_with(
            op,
            "runs",
            |id| {
                browse::touch_repo(owner, repo);
                if browse::cache_only() {
                    return browse::finalize_cached_runs_list(owner, repo);
                }
                let repo_path = RepoPath::new(owner, repo);
                let api_path = repo_path.api_path("actions/runs?per_page=30");
                dispatch_or_err(
                    id,
                    Continuation::FetchingFirstPage {
                        path: repo_path.path("_actions/runs"),
                        is_org_fallback: false,
                    },
                    api::github_get(&api_path),
                )
            },
            |_| browse::err(ProviderError::not_found("not found")),
        )
    }

    #[route("/{owner}/{repo}/_actions/runs/{run_id}")]
    fn action_run(op: Op, owner: Owner, repo: Repo, run_id: u64) -> Option<ProviderResponse> {
        let _ = owner;
        let _ = repo;
        let run_id_str = run_id.to_string();
        dir_only_with(
            op,
            run_id_str,
            |_id| {
                ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                    entries: vec![mk_file("status"), mk_file("conclusion"), mk_file("log")],
                    exhaustive: true,
                }))
            },
            |_| Some(browse::err(ProviderError::not_found("not found"))),
        )
    }

    #[route("/{owner}/{repo}/_actions/runs/{run_id}/{file}")]
    fn action_run_file(
        op: Op,
        owner: Owner,
        repo: Repo,
        run_id: u64,
        file: RunFile,
    ) -> Option<ProviderResponse> {
        let owner = owner.as_str();
        let repo = repo.as_str();
        let repo_path = RepoPath::new(owner, repo);
        let run_id_str = run_id.to_string();
        let file_name = match file {
            RunFile::Status => "status",
            RunFile::Conclusion => "conclusion",
            RunFile::Log => "log",
        };
        file_only_with(
            op,
            file_name,
            |id| {
                browse::touch_repo(owner, repo);

                if file == RunFile::Log {
                    let log_cache_key =
                        repo_path.cache_path(&format!("actions/runs/{run_id_str}/log"));
                    if let Ok(Some(data)) = browse::get_cached(&log_cache_key) {
                        return ProviderResponse::Done(ActionResult::FileContent(data));
                    }
                    if browse::cache_only() {
                        return browse::err(ProviderError::not_found("not found in cache"));
                    }
                    let api_path = repo_path.api_path(&format!("actions/runs/{run_id_str}/logs"));
                    return dispatch_or_err(
                        id,
                        Continuation::FetchingRunLog {
                            path: repo_path.path(&format!("_actions/runs/{run_id_str}/log")),
                        },
                        api::github_get(&api_path),
                    );
                }

                let cache_key = repo_path.cache_path(&format!("actions/runs/{run_id_str}"));
                if let Ok(Some(data)) = browse::get_cached(&cache_key) {
                    return browse::serve_run_file(&data, file);
                }
                if browse::cache_only() {
                    return browse::err(ProviderError::not_found("not found in cache"));
                }

                let api_path = repo_path.api_path(&format!("actions/runs/{run_id_str}"));
                dispatch_or_err(
                    id,
                    Continuation::FetchingResource {
                        path: repo_path.path(&format!("_actions/runs/{run_id_str}/{file_name}")),
                    },
                    api::github_get(&api_path),
                )
            },
            |_| browse::err(ProviderError::not_found("not found")),
        )
    }

    #[route("/{owner}/{repo}/{ns}/{filter}")]
    fn resource_filter(
        op: Op,
        owner: Owner,
        repo: Repo,
        ns: ResourceKind,
        filter: StateFilter,
    ) -> Option<ProviderResponse> {
        let owner = owner.as_str();
        let repo = repo.as_str();
        let repo_path = RepoPath::new(owner, repo);
        dir_only_with(
            op,
            filter.as_ref(),
            |id| {
                browse::touch_repo(owner, repo);
                if browse::cache_only() {
                    return browse::finalize_cached_resource_list(owner, repo, ns, filter);
                }
                let resource_kind = ns.search_qualifier();
                let state_clause = match filter {
                    StateFilter::Open => "+state:open",
                    StateFilter::All => "",
                };
                let query = format!("repo:{owner}/{repo}+is:{resource_kind}{state_clause}");
                let filter_name = filter.as_ref();
                let ns_name = ns.as_ref();
                let api_path =
                    format!("/search/issues?q={query}&sort=created&order=desc&per_page=100");
                let path = repo_path.path(&format!("{ns_name}/{filter_name}"));
                dispatch_or_err(
                    id,
                    Continuation::FetchingFirstPage {
                        path,
                        is_org_fallback: false,
                    },
                    api::github_get(&api_path),
                )
            },
            |_| Some(browse::err(ProviderError::not_found("not found"))),
        )
    }

    #[route("/{owner}/{repo}/{ns}/{filter}/{number}")]
    fn resource(
        op: Op,
        owner: Owner,
        repo: Repo,
        ns: ResourceKind,
        filter: StateFilter,
        number: u64,
    ) -> Option<ProviderResponse> {
        let owner = owner.as_str();
        let repo = repo.as_str();
        let number_str = number.to_string();
        let ns_name = ns.as_ref();
        let filter_name = filter.as_ref();
        let repo_path = RepoPath::new(owner, repo);
        dir_only_with(
            op,
            number_str.as_str(),
            |_id| {
                browse::touch_repo(owner, repo);
                let mut files = vec![
                    mk_file("title"),
                    mk_file("body"),
                    mk_file("state"),
                    mk_file("user"),
                    mk_dir("comments"),
                ];
                if ns == ResourceKind::Prs {
                    files.push(mk_file("diff"));
                }
                ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                    entries: files,
                    exhaustive: true,
                }))
            },
            |id| {
                let api_resource = ns.api_path();
                let cache_key = repo_path.cache_path(&format!("{api_resource}/{number_str}"));
                let cached = browse::get_cached(&cache_key)
                    .map(|value| value.is_some())
                    .unwrap_or(false);
                if cached {
                    return Some(dir_entry(&number_str));
                }
                if browse::cache_only() {
                    return Some(ProviderResponse::Done(ActionResult::DirEntryOption(None)));
                }
                let api_path = repo_path.api_path(&format!("{api_resource}/{number_str}"));
                let full_path = repo_path.path(&format!("{ns_name}/{filter_name}/{number_str}"));
                Some(dispatch_or_err(
                    id,
                    Continuation::ValidatingResource {
                        path: full_path,
                        name: number_str.clone(),
                    },
                    api::github_get(&api_path),
                ))
            },
        )
    }

    #[route("/{owner}/{repo}/{ns}/{filter}/{number}/comments")]
    fn comments_dir(
        op: Op,
        owner: Owner,
        repo: Repo,
        ns: ResourceKind,
        filter: StateFilter,
        number: u64,
    ) -> Option<ProviderResponse> {
        let _ = (ns, filter); // used for routing constraint, not in logic
        let owner = owner.as_str();
        let repo = repo.as_str();
        let number_str = number.to_string();
        let repo_path = RepoPath::new(owner, repo);
        dir_only_with(
            op,
            "comments",
            |id| {
                browse::touch_repo(owner, repo);
                let cache_key = repo_path.cache_path(&format!("issues/{number_str}/comments"));
                if let Ok(Some(data)) = browse::get_cached(&cache_key) {
                    return browse::list_cached_comments(&data);
                }
                if browse::cache_only() {
                    return ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                        entries: vec![],
                        exhaustive: false,
                    }));
                }
                let ns_name = ns.as_ref();
                let filter_name = filter.as_ref();
                let api_path =
                    repo_path.api_path(&format!("issues/{number_str}/comments?per_page=100"));
                let path =
                    repo_path.path(&format!("{ns_name}/{filter_name}/{number_str}/comments"));
                dispatch_or_err(
                    id,
                    Continuation::FetchingComments { path },
                    api::github_get(&api_path),
                )
            },
            |_| Some(browse::err(ProviderError::not_found("not found"))),
        )
    }

    #[route("/{owner}/{repo}/{ns}/{filter}/{number}/comments/{idx}")]
    fn comment_file(
        op: Op,
        owner: Owner,
        repo: Repo,
        ns: ResourceKind,
        filter: StateFilter,
        number: u64,
        idx: u64,
    ) -> Option<ProviderResponse> {
        let owner = owner.as_str();
        let repo = repo.as_str();
        let number_str = number.to_string();
        let idx_str = idx.to_string();
        let idx_name = idx_str.as_str();
        let repo_path = RepoPath::new(owner, repo);
        file_only_with(
            op,
            idx_name,
            |id| {
                browse::touch_repo(owner, repo);
                let cache_key = repo_path.cache_path(&format!("issues/{number_str}/comments"));
                if let Ok(Some(data)) = browse::get_cached(&cache_key) {
                    return browse::serve_comment_file(&data, &idx_str);
                }
                if browse::cache_only() {
                    return browse::err(ProviderError::not_found("not found in cache"));
                }
                let ns_name = ns.as_ref();
                let filter_name = filter.as_ref();
                let api_path =
                    repo_path.api_path(&format!("issues/{number_str}/comments?per_page=100"));
                let path = repo_path.path(&format!(
                    "{ns_name}/{filter_name}/{number_str}/comments/{idx_str}"
                ));
                dispatch_or_err(
                    id,
                    Continuation::FetchingComments { path },
                    api::github_get(&api_path),
                )
            },
            |_| browse::err(ProviderError::not_found("not found")),
        )
    }

    #[route("/{owner}/{repo}/{ns}/{filter}/{number}/{file}")]
    fn resource_file(
        op: Op,
        owner: Owner,
        repo: Repo,
        ns: ResourceKind,
        filter: StateFilter,
        number: u64,
        file: ResourceFile,
    ) -> Option<ProviderResponse> {
        let owner = owner.as_str();
        let repo = repo.as_str();
        // Cross-field validation: diff only valid for PRs
        if file == ResourceFile::Diff && ns != ResourceKind::Prs {
            return None;
        }
        let number_str = number.to_string();
        let file_name = match file {
            ResourceFile::Title => "title",
            ResourceFile::Body => "body",
            ResourceFile::State => "state",
            ResourceFile::User => "user",
            ResourceFile::Diff => "diff",
        };
        let repo_path = RepoPath::new(owner, repo);
        file_only_with(
            op,
            file_name,
            |id| {
                browse::touch_repo(owner, repo);
                let api_resource = ns.api_path();
                let ns_name = ns.as_ref();
                let filter_name = filter.as_ref();

                // Diff requires separate fetch with different Accept header
                if file == ResourceFile::Diff {
                    let diff_cache_key = repo_path.cache_path(&format!("pulls/{number_str}/diff"));
                    if let Ok(Some(data)) = browse::get_cached(&diff_cache_key) {
                        return ProviderResponse::Done(ActionResult::FileContent(data));
                    }
                    if browse::cache_only() {
                        return browse::err(ProviderError::not_found("not found in cache"));
                    }
                    let url = repo_path.api_path(&format!("pulls/{number_str}"));
                    let path =
                        repo_path.path(&format!("{ns_name}/{filter_name}/{number_str}/diff"));
                    return dispatch_or_err(
                        id,
                        Continuation::FetchingDiff { path },
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

                let cache_key = repo_path.cache_path(&format!("{api_resource}/{number_str}"));

                if let Ok(Some(data)) = browse::get_cached(&cache_key) {
                    return browse::serve_resource_file(&data, file);
                }
                if browse::cache_only() {
                    return browse::err(ProviderError::not_found("not found in cache"));
                }

                let api_path = repo_path.api_path(&format!("{api_resource}/{number_str}"));
                let path =
                    repo_path.path(&format!("{ns_name}/{filter_name}/{number_str}/{file_name}"));
                dispatch_or_err(
                    id,
                    Continuation::FetchingResource { path },
                    api::github_get(&api_path),
                )
            },
            |_| browse::err(ProviderError::not_found("not found")),
        )
    }

    // --- Helpers ---

    fn namespace_handler(
        op: Op,
        owner: &str,
        repo: &str,
        ns: Namespace,
    ) -> Option<ProviderResponse> {
        let ns_name = ns.as_ref();
        let repo_path = RepoPath::new(owner, repo);
        dir_only_with(
            op,
            ns_name,
            |id| {
                browse::touch_repo(owner, repo);
                match ns {
                    Namespace::Issues | Namespace::Prs => {
                        ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                            entries: vec![mk_dir("_open"), mk_dir("_all")],
                            exhaustive: true,
                        }))
                    }
                    Namespace::Actions => {
                        ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                            entries: vec![mk_dir("runs")],
                            exhaustive: true,
                        }))
                    }
                    Namespace::Repo => dispatch_or_err(
                        id,
                        Continuation::DisowningRepo,
                        SingleEffect::GitOpenRepo(GitOpenRequest {
                            clone_url: repo_path.clone_url(),
                            cache_key: repo_path.github_cache_key(),
                        }),
                    ),
                }
            },
            |_| Some(browse::err(ProviderError::not_found("not found"))),
        )
    }

    fn list_owner(id: u64, owner: &str) -> ProviderResponse {
        if browse::cache_only() {
            return dispatch_or_err(
                id,
                Continuation::ListingCachedRepos {
                    path: owner.to_string(),
                    mode: CachedRepoListMode::Owner,
                },
                SingleEffect::GitListCachedRepos(GitCacheListRequest {
                    prefix: Some(github_owner_cache_prefix(owner)),
                }),
            );
        }
        // Check negative owner cache
        let is_negative = with_state(|state| {
            if let Some(&cached_tick) = state.negative_owners.get(owner) {
                let now = state.cache.current_tick();
                now.saturating_sub(cached_tick) < browse::NEGATIVE_OWNER_TTL
            } else {
                false
            }
        })
        .unwrap_or(false);
        if is_negative {
            return ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                entries: vec![],
                exhaustive: true,
            }));
        }
        // Return from repo list cache if fresh.
        let cached = with_state(|state| {
            if let Some((tick, repos)) = state.owner_repos_cache.get(owner) {
                let now = state.cache.current_tick();
                if now.saturating_sub(*tick) < OWNER_REPOS_CACHE_TTL {
                    return Some(repos.clone());
                }
            }
            None
        })
        .ok()
        .flatten();
        if let Some(repos) = cached {
            let entries: Vec<DirEntry> = repos.into_iter().map(mk_dir).collect();
            return ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                entries,
                exhaustive: false,
            }));
        }
        // Fetch owner profile to determine kind and repo count.
        let known_kind = with_state(|state| state.owner_kinds.get(owner).copied())
            .ok()
            .flatten();
        let api_path = match known_kind {
            Some(OwnerKind::Org) => format!("/orgs/{owner}"),
            _ => format!("/users/{owner}"),
        };
        dispatch_or_err(
            id,
            Continuation::FetchingOwnerProfile {
                path: owner.to_string(),
                is_org_fallback: known_kind == Some(OwnerKind::Org),
            },
            api::github_get(&api_path),
        )
    }
    fn lookup_repo(id: u64, owner: &str, repo: &str) -> ProviderResponse {
        let repo_path = RepoPath::new(owner, repo);
        if browse::cache_only() {
            return dispatch_or_err(
                id,
                Continuation::ListingCachedRepos {
                    path: repo_path.cache_key(),
                    mode: CachedRepoListMode::ValidateRepo,
                },
                SingleEffect::GitListCachedRepos(GitCacheListRequest {
                    prefix: Some(github_owner_cache_prefix(owner)),
                }),
            );
        }
        dispatch_or_err(
            id,
            Continuation::ValidatingRepo {
                path: repo_path.cache_key(),
            },
            api::github_get(&format!("/repos/{owner}/{repo}")),
        )
    }
}
