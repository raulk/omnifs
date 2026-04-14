//! github-provider: GitHub virtual filesystem provider for omnifs.
//!
//! Exposes GitHub resources (issues, PRs, actions, repository contents)
//! as a virtual filesystem using the omnifs provider WIT interface.
//! Uses an async continuation-based model with request coalescing and
//! LRU caching for API responses.

wit_bindgen::generate!({
    path: "../../wit",
    world: "provider",
});

mod api;
mod browse;
mod cache;
pub(crate) mod path;

use hashbrown::HashMap;
use omnifs::provider::types::*;
use std::cell::RefCell;

type ProviderResult<T> = Result<T, String>;

struct GithubProvider;

thread_local! {
    static STATE: RefCell<Option<ProviderState>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnerKind {
    User,
    Org,
}

struct ProviderState {
    pending: HashMap<u64, Continuation>,
    cache: cache::Cache,
    /// Negative cache for owners that returned 404 from both /users/ and /orgs/.
    negative_owners: HashMap<String, u64>,
    /// Cached owner type (user vs org) to skip the 404 fallback on subsequent listings.
    owner_kinds: HashMap<String, OwnerKind>,
    /// Cached repo lists per owner, with the tick at which they were fetched.
    owner_repos_cache: HashMap<String, (u64, Vec<String>)>,
    /// Last seen X-RateLimit-Remaining value from GitHub API responses.
    rate_limit_remaining: Option<u32>,
    cache_only: bool,
    active_repos: HashMap<String, u64>,
    event_etags: HashMap<String, String>,
    /// Host cache invalidation prefixes collected from events, to emit on next timer tick.
    pending_host_invalidations: Vec<String>,
}

const OWNER_REPOS_CACHE_TTL: u64 = 120;

enum Continuation {
    ListingCachedRepos {
        path: String,
        mode: CachedRepoListMode,
    },
    FetchingFirstPage {
        path: String,
        /// True when this is the org-endpoint fallback (second attempt).
        is_org_fallback: bool,
    },
    /// Fetching owner profile (/users/ or /orgs/) to determine kind and repo count.
    FetchingOwnerProfile {
        path: String,
        is_org_fallback: bool,
    },
    /// All repo listing pages dispatched in parallel.
    FetchingRepoPages {
        path: String,
    },
    /// Page 1 returned; now fetching remaining pages in parallel (search API).
    FetchingRemainingPages {
        path: String,
        first_page_items: Vec<serde_json::Value>,
    },
    FetchingResource {
        path: String,
    },
    /// Validating that a repo exists via GET /repos/{owner}/{repo}.
    ValidatingRepo {
        path: String,
    },
    /// Validating that an issue/PR number exists via direct API lookup.
    ValidatingResource {
        path: String,
        name: String,
    },
    FetchingComments {
        path: String,
    },
    /// Opening a repo to disown the subtree to FUSE passthrough.
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

enum CachedRepoListMode {
    Root,
    Owner,
    ValidateRepo,
}

/// Access STATE, returning a provider error if not initialized.
fn with_state<F, R>(f: F) -> ProviderResult<R>
where
    F: FnOnce(&mut ProviderState) -> R,
{
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        match borrow.as_mut() {
            Some(state) => Ok(f(state)),
            None => Err("provider not initialized".to_string()),
        }
    })
}

// with_state covers both mutable and shared access (cache.get needs &mut self for LRU refresh)

impl exports::omnifs::provider::lifecycle::Guest for GithubProvider {
    fn initialize(_config: Vec<u8>) -> ProviderResponse {
        STATE.with(|s| {
            *s.borrow_mut() = Some(ProviderState {
                pending: HashMap::new(),
                cache: cache::Cache::new(),
                negative_owners: HashMap::new(),
                owner_kinds: HashMap::new(),
                owner_repos_cache: HashMap::new(),
                rate_limit_remaining: None,
                cache_only: false,
                active_repos: HashMap::new(),
                event_etags: HashMap::new(),
                pending_host_invalidations: Vec::new(),
            });
        });
        let _ = with_state(|state| state.cache.advance_tick());

        ProviderResponse::Done(ActionResult::ProviderInitialized(ProviderInfo {
            name: "github-provider".to_string(),
            version: "0.1.0".to_string(),
            description: "GitHub API provider for omnifs".to_string(),
        }))
    }

    fn shutdown() {
        STATE.with(|s| {
            *s.borrow_mut() = None;
        });
    }

    fn get_config_schema() -> ConfigSchema {
        ConfigSchema { fields: vec![] }
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
}

impl exports::omnifs::provider::browse::Guest for GithubProvider {
    fn lookup_child(id: u64, parent_path: String, name: String) -> ProviderResponse {
        browse::lookup_child(id, &parent_path, &name)
    }

    fn list_children(id: u64, path: String) -> ProviderResponse {
        browse::list_children(id, &path)
    }

    fn read_file(id: u64, path: String) -> ProviderResponse {
        browse::read_file(id, &path)
    }

    fn open_file(_id: u64, _path: String) -> ProviderResponse {
        ProviderResponse::Done(ActionResult::FileOpened(1))
    }

    fn read_chunk(_id: u64, _handle: u64, _offset: u64, _len: u32) -> ProviderResponse {
        ProviderResponse::Done(ActionResult::FileChunk(vec![]))
    }

    fn close_file(_handle: u64) {}
}

impl exports::omnifs::provider::resume::Guest for GithubProvider {
    fn resume(id: u64, effect_outcome: EffectResult) -> ProviderResponse {
        browse::resume(id, effect_outcome)
    }

    fn cancel(id: u64) {
        let _ = with_state(|state| {
            state.pending.remove(&id);
        });
    }
}

impl exports::omnifs::provider::reconcile::Guest for GithubProvider {
    fn plan_mutations(_id: u64, _changes: Vec<FileChange>) -> ProviderResponse {
        ProviderResponse::Done(ActionResult::Err(
            "mutations are not implemented".to_string(),
        ))
    }

    fn execute(_id: u64, _mutation: PlannedMutation) -> ProviderResponse {
        ProviderResponse::Done(ActionResult::Err(
            "mutations are not implemented".to_string(),
        ))
    }

    fn fetch_resource(_id: u64, _resource_path: String) -> ProviderResponse {
        ProviderResponse::Done(ActionResult::Err(
            "fetch_resource is not implemented".to_string(),
        ))
    }

    fn list_scope(_id: u64, _scope: String) -> ProviderResponse {
        ProviderResponse::Done(ActionResult::Err(
            "list_scope is not implemented".to_string(),
        ))
    }
}

impl exports::omnifs::provider::notify::Guest for GithubProvider {
    fn on_event(id: u64, event: ProviderEvent) -> ProviderResponse {
        match event {
            ProviderEvent::TimerTick => browse::timer_tick(id),
            _ => ProviderResponse::Done(ActionResult::Ok),
        }
    }
}

export!(GithubProvider);

// Re-export is_safe_segment for backward compatibility
pub use crate::path::is_safe_segment;
