# omnifs SDK and provider router implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract shared provider infrastructure into an `omnifs-sdk` crate. Providers declare themselves with `#[omnifs::provider]` on an impl block, embed `#[path("...")]` route handlers directly alongside lifecycle methods, and receive typed captures via `FromStr`. The SDK handles TOML config deserialization, state management, WIT trait glue, and the continuation dispatch loop. Providers contain only domain logic.

## End-state provider API

A complete provider looks like this:

```rust
// providers/github/src/lib.rs

use omnifs_sdk::prelude::*;

mod api;
mod cache;
mod resume;

#[derive(Deserialize)]
pub struct Config { /* bare keys from [config] section */ }

pub struct State {
    pub cache: cache::Cache,
    pub rate_limit_remaining: Option<u32>,
    pub cache_only: bool,
    pub active_repos: HashMap<String, u64>,
    // ...
}

pub enum Continuation {
    FetchingResource { path: String },
    ValidatingRepo { path: String },
    // ...
}

#[omnifs::provider]
impl GithubProvider {
    fn init(config: Config) -> (State, ProviderInfo) {
        let state = State { cache: cache::Cache::new(128), ... };
        let info = ProviderInfo {
            name: "github-provider".into(),
            version: "0.1.0".into(),
            description: "GitHub API provider for omnifs".into(),
        };
        (state, info)
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["api.github.com".into()],
            auth_types: vec!["token".into()],
            max_memory_mb: 128,
            needs_git: true,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 60,
        }
    }

    fn resume(id: u64, cont: Continuation, outcome: EffectResult) -> ProviderResponse {
        match cont {
            Continuation::FetchingResource { path } => resume::resource(&path, &outcome),
            // ...
        }
    }

    fn on_event(id: u64, event: ProviderEvent) -> ProviderResponse {
        match event {
            ProviderEvent::TimerTick => browse::timer_tick(id),
            _ => ProviderResponse::Done(ActionResult::Ok),
        }
    }

    // --- Routes ---

    #[path("/")]
    fn root(op: Op) -> Option<ProviderResponse> {
        match op {
            Op::Lookup(_) => None,
            Op::List(id) => Some(dispatch(id, Continuation::ListingCachedRepos { ... }, ...)),
            Op::Read(_) => Some(err("not found")),
        }
    }

    #[path("/{owner}/{repo}")]
    fn repo(op: Op, owner: &str, repo: &str) -> Option<ProviderResponse> {
        if !is_safe_segment(owner) || !is_safe_segment(repo) { return None; }
        match op {
            Op::Lookup(id) => { /* validate via API */ }
            Op::List(_) => { /* list namespaces */ }
            Op::Read(_) => Some(err("not found")),
        }
    }

    #[path("/{owner}/{repo}/_actions/runs/{run_id}")]
    fn action_run(op: Op, owner: &str, repo: &str, run_id: u64) -> Option<ProviderResponse> {
        if !is_safe_segment(owner) || !is_safe_segment(repo) { return None; }
        match op {
            Op::Lookup(_) => Some(dir_entry(&run_id.to_string())),
            Op::List(_) => { /* list run files */ }
            Op::Read(_) => Some(err("not found")),
        }
    }

    #[path("/{owner}/{repo}/{ns}/{filter}/{number}")]
    fn resource(op: Op, owner: &str, repo: &str, ns: ResourceKind, filter: StateFilter, number: u64) -> Option<ProviderResponse> {
        if !is_safe_segment(owner) || !is_safe_segment(repo) { return None; }
        // ns, filter, number already parsed via FromStr. Parse failure = None (fallthrough).
        match op {
            Op::Lookup(id) => { /* validate resource */ }
            Op::List(_) => { /* list resource files */ }
            Op::Read(_) => Some(err("not found")),
        }
    }

    #[path("/{owner}/{repo}/_repo/{*tree_path}")]
    fn repo_tree(op: Op, owner: &str, repo: &str, tree_path: &str) -> Option<ProviderResponse> {
        if !is_safe_segment(owner) || !is_safe_segment(repo) { return None; }
        if !is_safe_tree_path(tree_path) { return None; }
        // All ops disown to git passthrough.
        touch_repo(owner, repo);
        Some(dispatch(op.id(), Continuation::DisowningRepo, ...))
    }

    // --- Helpers (no #[path], kept as inherent methods) ---

    fn namespace_handler(op: Op, owner: &str, repo: &str, ns: Namespace) -> Option<ProviderResponse> {
        // ...
    }
}
```

**What `#[omnifs::provider]` generates from this impl block:**

1. `struct GithubProvider;`
2. `thread_local!` state storage + `with_state()`, `dispatch()`, `dispatch_batch()` accessors
3. `lifecycle::Guest` impl: TOML deserialization (inferred from `init(config: Config) -> (State, ProviderInfo)`), capabilities delegation, shutdown
4. `browse::Guest` impl: path joining for lookup, `dispatch(Op, &path)` delegation to the generated route dispatch chain
5. `resume::Guest` impl: continuation retrieval from pending map, delegation to `resume()`
6. `notify::Guest` impl: delegates to `on_event()` if present, default stub if absent
7. Default `reconcile::Guest` stubs
8. `export!(GithubProvider);`

**Typed captures via `FromStr`:** parameters that are not `&str` are parsed from the segment string using `.parse::<T>().ok()?`. Failed parse returns `None` (fallthrough to next route). This eliminates `is_numeric()` checks, `from_dir_name()` calls, and manual validation preambles.

| Parameter type | Generated code | Example |
|---|---|---|
| `&str` | pass segment directly | `owner: &str` |
| `u64` | `segments[i].parse::<u64>().ok()?` | `run_id: u64` |
| `ResourceKind` | `segments[i].parse::<ResourceKind>().ok()?` | `ns: ResourceKind` |
| `StateFilter` | `segments[i].parse::<StateFilter>().ok()?` | `filter: StateFilter` |
| `RunFile` | `segments[i].parse::<RunFile>().ok()?` | `file: RunFile` |

Types like `ResourceKind`, `StateFilter`, `RunFile`, `ResourceFile` implement `FromStr`. Their `from_str` replaces the current `from_dir_name`/`from_name` methods:

```rust
impl FromStr for ResourceKind {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s { "_issues" => Ok(Self::Issues), "_prs" => Ok(Self::Prs), _ => Err(()) }
    }
}

impl FromStr for StateFilter {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s { "_open" => Ok(Self::Open), "_all" => Ok(Self::All), _ => Err(()) }
    }
}

impl FromStr for RunFile {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "status" => Ok(Self::Status), "conclusion" => Ok(Self::Conclusion),
            "log" => Ok(Self::Log), _ => Err(()),
        }
    }
}
```

**Root path:** `#[path("/")]` matches when the input path is empty. The template `/` is the root; segments after `/` form the path structure.

---

## Architecture

Two crates:

- **`crates/omnifs-sdk`** (runtime): runs `wit_bindgen::generate!` once and re-exports all WIT types (`ProviderResponse`, `ActionResult`, `DirEntry`, etc.) and trait definitions (`exports::omnifs::provider::browse::Guest`, etc.). Also provides: `Op` enum, helper functions (`err`, `dir_entry`, `file_entry`, `mk_dir`, `mk_file`), generic `Cache` type, `extract_http_body`, re-export of `#[omnifs::provider]` and `#[path]` macros.
- **`crates/omnifs-sdk-macros`** (proc macro): implements `#[omnifs::provider]` (processes impl block, generates WIT trait impls + state management + dispatch/dispatch_batch + route dispatch + `export!()`) and `#[path("...")]` (marker attribute consumed by the provider macro).

The SDK crate depends on `omnifs-sdk-macros`, `wit-bindgen`, `serde`, `toml`, and `hashbrown`. Providers depend only on `omnifs-sdk` (no direct `wit-bindgen` dependency).

**WIT bindings live in the SDK.** The SDK runs `wit_bindgen::generate!` once and publicly re-exports the generated types and traits. This is the same pattern used by the `wasi` crate. Providers import WIT types from `omnifs_sdk::` instead of generating their own. This eliminates the 34K-line `bindings.rs` duplication across providers. The `#[omnifs::provider]` macro generates `impl omnifs_sdk::exports::omnifs::provider::browse::Guest for MyProvider` and emits the `export!()` call, both referencing SDK paths.

```rust
// crates/omnifs-sdk/src/lib.rs
wit_bindgen::generate!({
    world: "provider",
    path: "../../wit",
    pub_export_macro: true,  // makes export!() available to dependents
});

// wit_bindgen::generate! already emits pub mod omnifs and pub mod exports.
// Re-export the types at a convenient path for the prelude.
// Providers use: use omnifs::prelude::*; (where omnifs is the renamed dep)
```

**Proc macro crate runs on the host at compile time.** It uses `syn`/`quote` and is never compiled to wasm. The SDK runtime crate compiles to wasm32-wasip1 as a transitive dependency of providers.

---

## How the macro processes the impl block

The `#[omnifs::provider]` macro on `impl GithubProvider { ... }` classifies each method:

| Method signature | Classification | Macro action |
|---|---|---|
| `fn init(config: T) -> (S, ProviderInfo)` | Lifecycle init | Infers Config=T, State=S. Generates TOML deser + state init + `ProviderInitialized(info)` return in `lifecycle::Guest::initialize` |
| `fn capabilities() -> RequestedCapabilities` | Lifecycle caps | Delegates in `lifecycle::Guest::capabilities` |
| `fn resume(id: u64, cont: C, outcome: EffectResult) -> ProviderResponse` | Resume | Infers Continuation=C. Generates pending map retrieval + delegation in `resume::Guest::resume` |
| `fn on_event(id: u64, event: ProviderEvent) -> ProviderResponse` | Notify (optional) | Delegates in `notify::Guest::on_event` |
| `#[path("...")]` methods | Route handlers | Collected into dispatch chain for `browse::Guest` |
| Everything else | Helpers | Kept as inherent methods on the struct |

**Route dispatch generation:** the macro collects all `#[path]` methods in source order and generates:

```rust
fn __dispatch(op: omnifs_sdk::Op, path: &str) -> Option<ProviderResponse> {
    None
        .or_else(|| __match_root(op, path))
        .or_else(|| __match_owner(op, path))
        .or_else(|| __match_repo(op, path))
        // ... all handlers in source order
}
```

Each `__match_<name>` wrapper splits the path, checks segment count, compares literals, parses captures via `FromStr` where the type is not `&str`, and calls the original method.

**Match wrapper generation example:**

For:
```rust
#[path("/{owner}/{repo}/_actions/runs/{run_id}/{file}")]
fn action_run_file(op: Op, owner: &str, repo: &str, run_id: u64, file: RunFile) -> Option<ProviderResponse> { ... }
```

Generates:
```rust
fn __match_action_run_file(op: omnifs_sdk::Op, path: &str) -> Option<ProviderResponse> {
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() != 6 { return None; }
    if segments[2] != "_actions" { return None; }
    if segments[3] != "runs" { return None; }
    let owner: &str = segments[0];
    let repo: &str = segments[1];
    let run_id: u64 = segments[4].parse().ok()?;
    let file: RunFile = segments[5].parse().ok()?;
    GithubProvider::action_run_file(op, owner, repo, run_id, file)
}
```

For rest captures (`{*tree_path}`), a template with N fixed segments before the rest generates `if segments.len() < N + 1` (ensuring at least one rest segment):

```rust
fn __match_repo_tree(op: omnifs_sdk::Op, path: &str) -> Option<ProviderResponse> {
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() < 4 { return None; }  // 3 fixed + at least 1 rest
    if segments[2] != "_repo" { return None; }
    let owner: &str = segments[0];
    let repo: &str = segments[1];
    let rest_offset: usize = segments[..3].iter().map(|s| s.len() + 1).sum();
    let tree_path: &str = &path[rest_offset..];
    GithubProvider::repo_tree(op, owner, repo, tree_path)
}
```

For `#[path("/")]` (root):
```rust
fn __match_root(op: omnifs_sdk::Op, path: &str) -> Option<ProviderResponse> {
    if !path.is_empty() { return None; }
    GithubProvider::root(op)
}
```

**Generated browse::Guest impl:**

```rust
impl exports::omnifs::provider::browse::Guest for GithubProvider {
    fn lookup_child(id: u64, parent_path: String, name: String) -> ProviderResponse {
        let path = if parent_path.is_empty() { name } else { format!("{parent_path}/{name}") };
        __dispatch(omnifs_sdk::Op::Lookup(id), &path)
            .unwrap_or_else(|| ProviderResponse::Done(ActionResult::DirEntryOption(None)))
    }

    fn list_children(id: u64, path: String) -> ProviderResponse {
        __dispatch(omnifs_sdk::Op::List(id), &path)
            .unwrap_or_else(|| ProviderResponse::Done(ActionResult::Err("not found".into())))
    }

    fn read_file(id: u64, path: String) -> ProviderResponse {
        __dispatch(omnifs_sdk::Op::Read(id), &path)
            .unwrap_or_else(|| ProviderResponse::Done(ActionResult::Err("not found".into())))
    }

    fn open_file(_: u64, _: String) -> ProviderResponse {
        ProviderResponse::Done(ActionResult::FileOpened(1))
    }
    fn read_chunk(_: u64, _: u64, _: u64, _: u32) -> ProviderResponse {
        ProviderResponse::Done(ActionResult::FileChunk(vec![]))
    }
    fn close_file(_: u64) {}
}
```

**Generated lifecycle::Guest impl:**

```rust
impl exports::omnifs::provider::lifecycle::Guest for GithubProvider {
    fn initialize(config_bytes: Vec<u8>) -> ProviderResponse {
        let config_str = match core::str::from_utf8(&config_bytes) {
            Ok(s) => s,
            Err(e) => return ProviderResponse::Done(ActionResult::Err(format!("invalid UTF-8: {e}"))),
        };
        // Config type inferred from fn init(config: Config) -> (State, ProviderInfo)
        let config: Config = match toml::from_str(config_str) {
            Ok(c) => c,
            Err(e) => return ProviderResponse::Done(ActionResult::Err(format!("config error: {e}"))),
        };
        let (state, info) = GithubProvider::init(config);
        STATE.with(|s| {
            *s.borrow_mut() = Some(omnifs_sdk::__internal::StateWrapper {
                inner: state,
                pending: hashbrown::HashMap::new(),
            });
        });
        ProviderResponse::Done(ActionResult::ProviderInitialized(info))
    }

    fn capabilities() -> RequestedCapabilities {
        GithubProvider::capabilities()
    }

    fn shutdown() {
        STATE.with(|s| *s.borrow_mut() = None);
    }

    fn get_config_schema() -> ConfigSchema {
        ConfigSchema { fields: vec![] }
    }
}
```

**Generated state management:**

```rust
thread_local! {
    static STATE: core::cell::RefCell<Option<omnifs_sdk::__internal::StateWrapper<State, Continuation>>>
        = const { core::cell::RefCell::new(None) };
}

// Available to all methods in the provider crate.
// with_state: access provider's custom state.
pub(crate) fn with_state<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut State) -> R,
{
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        match borrow.as_mut() {
            Some(wrapper) => Ok(f(&mut wrapper.inner)),
            None => Err("provider not initialized".to_string()),
        }
    })
}

// with_pending: access the continuation pending map.
// Needed by browse submodules (files.rs, resources.rs) that call
// dispatch() to store continuations during resume handlers.
pub(crate) fn with_pending<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut hashbrown::HashMap<u64, Continuation>) -> R,
{
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        match borrow.as_mut() {
            Some(wrapper) => Ok(f(&mut wrapper.pending)),
            None => Err("provider not initialized".to_string()),
        }
    })
}
```

**Generated resume::Guest impl:**

```rust
impl exports::omnifs::provider::resume::Guest for GithubProvider {
    fn resume(id: u64, outcome: EffectResult) -> ProviderResponse {
        let cont = match STATE.with(|s| {
            let mut borrow = s.borrow_mut();
            borrow.as_mut().and_then(|w| w.pending.remove(&id))
        }) {
            Some(c) => c,
            None => return omnifs_sdk::err("no pending continuation"),
        };
        GithubProvider::resume(id, cont, outcome)
    }

    fn cancel(id: u64) {
        STATE.with(|s| {
            if let Some(w) = s.borrow_mut().as_mut() {
                w.pending.remove(&id);
            }
        });
    }
}
```

---

## SDK runtime contents

### `omnifs_sdk::prelude`

Re-exports everything a provider needs in a single `use`:

```rust
pub use crate::Op;
pub use crate::helpers::{err, dir_entry, file_entry, mk_dir, mk_file};
pub use crate::http::extract_http_body;
pub use crate::cache::Cache;
pub use omnifs_sdk_macros::{provider, path};  // invoked as #[omnifs::provider], #[path] via the renamed dep
pub use serde::Deserialize;
pub use hashbrown::HashMap;

// WIT types (generated once in the SDK, re-exported to all providers):
pub use crate::omnifs::provider::types::{
    ProviderResponse, ActionResult, DirEntry, DirListing, EntryKind,
    SingleEffect, EffectResult, SingleEffectResult,
    HttpRequest, HttpResponse, Header,
    GitOpenRequest, GitCacheListRequest,
    RequestedCapabilities, ConfigSchema, ConfigField, ProviderEvent,
    ProviderInfo, LogEntry, LogLevel,
    KvSetRequest, PlannedMutation, FileChange,
};

// Note: dispatch and dispatch_batch are NOT re-exported here.
// They are generated per-provider by the #[omnifs::provider] macro
// as module-level functions that close over the provider's STATE.
```

### `omnifs_sdk::Op`

```rust
#[derive(Clone, Copy, Debug)]
pub enum Op {
    Lookup(u64),
    List(u64),
    Read(u64),
}

impl Op {
    pub fn id(&self) -> u64 {
        match self { Op::Lookup(id) | Op::List(id) | Op::Read(id) => *id }
    }
}
```

### `omnifs_sdk::helpers`

```rust
pub fn err(msg: &str) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::Err(msg.to_string()))
}

pub fn dir_entry(name: &str) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::DirEntryOption(Some(DirEntry {
        name: name.to_string(),
        kind: EntryKind::Directory,
        size: None,
        projected_files: None,
    })))
}

pub fn file_entry(name: &str) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::DirEntryOption(Some(DirEntry {
        name: name.to_string(),
        kind: EntryKind::File,
        size: Some(4096),
        projected_files: None,
    })))
}

pub fn mk_dir(name: impl Into<String>) -> DirEntry {
    DirEntry { name: name.into(), kind: EntryKind::Directory, size: None, projected_files: None }
}

pub fn mk_file(name: impl Into<String>) -> DirEntry {
    DirEntry { name: name.into(), kind: EntryKind::File, size: Some(4096), projected_files: None }
}
```

### `dispatch` and `dispatch_batch` (generated, not in SDK)

These functions need the provider's pending `HashMap`, which lives in the `STATE` thread-local. The SDK does NOT export them. Instead, the `#[omnifs::provider]` macro generates them as module-level `pub(crate)` functions alongside `with_state`:

```rust
// Generated by #[omnifs::provider]:
pub(crate) fn dispatch(id: u64, cont: Continuation, effect: SingleEffect) -> ProviderResponse {
    let _ = with_pending(|pending| pending.insert(id, cont));
    ProviderResponse::Effect(effect)
}

pub(crate) fn dispatch_batch(id: u64, cont: Continuation, effects: Vec<SingleEffect>) -> ProviderResponse {
    let _ = with_pending(|pending| pending.insert(id, cont));
    ProviderResponse::Batch(effects)
}
```

Because these are generated at the crate root level, they shadow any other `dispatch` in scope. Handler code calls `dispatch(id, cont, effect)` naturally; no import or rewiring needed. The macro does NOT perform call-site rewriting inside method bodies.

The existing `browse::dispatch` in both providers calls `with_state(|s| s.pending.insert(id, cont))`. After migration, existing browse submodules (`files.rs`, `resources.rs`, `events.rs`) that call `browse::dispatch` or `super::dispatch` continue to work because: (a) the generated crate-root `dispatch` shadows the old one, or (b) the browse module's `dispatch` is updated to delegate to the generated one. During migration, the simplest path is to keep the `browse::dispatch` function and have it call `crate::with_pending(|p| p.insert(id, cont))`.

### `omnifs_sdk::http`

```rust
pub fn extract_http_body(result: &SingleEffectResult) -> Result<&[u8], ProviderResponse> {
    match result {
        SingleEffectResult::HttpResponse(resp) if resp.status < 400 => Ok(&resp.body),
        SingleEffectResult::HttpResponse(resp) => {
            Err(err(&format!("HTTP {}", resp.status)))
        }
        SingleEffectResult::EffectError(e) => {
            Err(err(&format!("effect error: {}", e.message)))
        }
        _ => Err(err("unexpected effect result type")),
    }
}
```

### `omnifs_sdk::cache`

Generic tick-based LRU cache extracted from the GitHub provider's `cache.rs`. Usable by any provider.

### `omnifs_sdk::__internal`

```rust
pub struct StateWrapper<S, C> {
    pub inner: S,
    pub pending: hashbrown::HashMap<u64, C>,
}
```

---

## File structure

| Action | Path | Responsibility |
|--------|------|----------------|
| Create | `crates/omnifs-sdk/Cargo.toml` | SDK runtime crate |
| Create | `crates/omnifs-sdk/src/lib.rs` | Op, re-exports |
| Create | `crates/omnifs-sdk/src/prelude.rs` | Single-use import for providers |
| Create | `crates/omnifs-sdk/src/helpers.rs` | err, dir_entry, file_entry, mk_dir, mk_file |
| (none) | `dispatch`/`dispatch_batch` | Generated per-provider by macro, not in SDK |
| Create | `crates/omnifs-sdk/src/http.rs` | extract_http_body |
| Create | `crates/omnifs-sdk/src/cache.rs` | Generic LRU cache |
| Create | `crates/omnifs-sdk-macros/Cargo.toml` | Proc macro crate |
| Create | `crates/omnifs-sdk-macros/src/lib.rs` | #[omnifs::provider], #[path] |
| Modify | `Cargo.toml` (workspace root) | Add SDK crates to workspace |
| Modify | `providers/github/Cargo.toml` | Replace deps with `omnifs-sdk` |
| Rewrite | `providers/github/src/lib.rs` | `#[omnifs::provider] impl` with embedded routes |
| Delete | `providers/github/src/path.rs` | Replaced by typed route captures |
| Delete | `providers/github/src/browse/routing.rs` | Replaced by route handlers |
| Modify | `providers/github/src/browse/mod.rs` | Remove routing re-exports, keep resume + helpers |
| Modify | `providers/dns/Cargo.toml` | Replace deps with `omnifs-sdk` |
| Rewrite | `providers/dns/src/lib.rs` | `#[omnifs::provider] impl` with embedded routes |
| Create | `providers/dns/src/types.rs` | RecordType (moved from path.rs) |
| Delete | `providers/dns/src/path.rs` | Replaced by typed route captures |
| Delete | `providers/dns/src/browse/routing.rs` | Replaced by route handlers |
| Modify | `providers/dns/src/browse/mod.rs` | Remove routing re-exports, keep resume + helpers |
| Rewrite | `providers/test/src/lib.rs` | Minimal `#[omnifs::provider] impl` |
| Modify | `justfile` | Add SDK to check/test commands |
| Modify | `.github/workflows/ci.yml` | Add SDK to CI |

---

### Task 1: Create the SDK macros crate

**Files:**
- Create: `crates/omnifs-sdk-macros/Cargo.toml`
- Create: `crates/omnifs-sdk-macros/src/lib.rs`

- [ ] **Step 1: Create manifest**

```toml
# crates/omnifs-sdk-macros/Cargo.toml
[package]
name = "omnifs-sdk-macros"
version = "0.1.0"
edition = "2024"
description = "Proc macros for the omnifs provider SDK"
license = "MIT OR Apache-2.0"

[lib]
proc-macro = true

[dependencies]
syn = { version = "2", features = ["full", "parsing", "extra-traits"] }
quote = "1"
proc-macro2 = "1"

[lints.rust]
unsafe_code = "deny"

[lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
must_use_candidate = "allow"
module_name_repetitions = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"
```

- [ ] **Step 2: Implement the macros**

The crate exports two proc macros:

1. `#[omnifs::provider]` (attribute macro on `impl TypeName`): the main macro that processes the impl block, classifies methods, generates WIT glue.
2. `#[path("...")]` (attribute macro on methods): marker consumed by the provider macro. If used outside an `#[omnifs::provider]` impl, emits a compile error.

The implementation parses the impl block with `syn`, collects methods by classification, generates match wrappers for `#[path]` methods with `FromStr` support, and emits all WIT trait implementations.

Template parsing, match wrapper generation, and dispatch chain generation follow the patterns described in the "How the macro processes the impl block" section above.

**Compile-time validation the macro performs:**
- `#[path]` parameter names match template capture names (emit `compile_error!` on mismatch)
- Template captures appear in the same order as function parameters (after `op: Op`)
- `init`, `capabilities`, and `resume` methods are present (emit `compile_error!` if missing)
- No duplicate path templates
- Rest capture `{*name}` is the last segment in its template

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p omnifs-sdk-macros`

---

### Task 2: Create the SDK runtime crate

**Files:**
- Create: `crates/omnifs-sdk/Cargo.toml`
- Create: `crates/omnifs-sdk/src/lib.rs`
- Create: `crates/omnifs-sdk/src/prelude.rs`
- Create: `crates/omnifs-sdk/src/helpers.rs`
- Create: `crates/omnifs-sdk/src/http.rs`
- Create: `crates/omnifs-sdk/src/cache.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Create manifest**

```toml
# crates/omnifs-sdk/Cargo.toml
[package]
name = "omnifs-sdk"
version = "0.1.0"
edition = "2024"
description = "SDK for building omnifs providers"
license = "MIT OR Apache-2.0"

[dependencies]
omnifs-sdk-macros = { path = "../omnifs-sdk-macros" }
wit-bindgen = "0.41"
hashbrown = "0.15"
serde = { version = "1", features = ["derive"] }
toml = "0.8"

[lints.rust]
unsafe_code = "deny"

[lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
must_use_candidate = "allow"
module_name_repetitions = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"
```

- [ ] **Step 2: Write the SDK modules**

Write `lib.rs`, `prelude.rs`, `helpers.rs`, `http.rs`, `cache.rs` as described in the "SDK runtime contents" section above. The `Op` enum, helper functions, HTTP utilities, and cache are the concrete implementations. `dispatch`/`dispatch_batch` are NOT in the SDK; they are generated per-provider by the macro.

- [ ] **Step 3: Add both crates to the workspace**

The `crates/*` glob in root `Cargo.toml` already covers new crates under `crates/`. Verify:

Run: `cargo metadata --no-deps --format-version 1 | jq '.packages[].name' | sort`

Expected: `omnifs-sdk` and `omnifs-sdk-macros` appear.

- [ ] **Step 4: Verify compilation**

Run: `cargo check -p omnifs-sdk`

- [ ] **Step 5: Commit**

```bash
git add crates/omnifs-sdk/ crates/omnifs-sdk-macros/
git commit -m "feat(sdk): add omnifs provider SDK with proc macro routing"
```

---

### Task 3: Migrate GitHub provider

**Files:**
- Modify: `providers/github/Cargo.toml`
- Rewrite: `providers/github/src/lib.rs`
- Create: `providers/github/src/types.rs`
- Modify: `providers/github/src/path.rs` (strip down to FsPath + parse only; remove types that moved to types.rs)
- Delete: `providers/github/src/browse/routing.rs`
- Modify: `providers/github/src/browse/mod.rs`
- Modify: `providers/github/src/browse/files.rs` (update imports)
- Modify: `providers/github/src/browse/resources.rs` (update imports)
- Modify: `providers/github/src/browse/events.rs` (update imports)

- [ ] **Step 1: Update dependencies**

Replace `wit-bindgen` with `omnifs-sdk` (WIT bindings come from SDK). The dependency is renamed to `omnifs` so that `#[omnifs::provider]` and `#[omnifs::router]` paths resolve:

```toml
[dependencies]
omnifs = { package = "omnifs-sdk", path = "../../crates/omnifs-sdk" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
hashbrown = "0.15"
rc-zip-sync = { version = "4", default-features = false, features = ["deflate"] }
```

- [ ] **Step 2: Add `FromStr` impls to domain types**

The types `ResourceKind`, `StateFilter`, `ResourceFile`, `RunFile`, `Namespace` move from `path.rs` to a new `types.rs` module, along with `is_safe_segment` and `is_safe_tree_path`. Each gets a `FromStr` implementation (replacing `from_dir_name`/`from_name`). The existing `from_dir_name`/`from_name` methods can be kept as aliases or removed.

`FsPath` and `FsPath::parse` remain in a stripped-down `path.rs` for now. The resume submodules (`browse/files.rs`, `browse/resources.rs`, `browse/events.rs`) call `FsPath::parse` on stored path strings to recover typed components during continuation handling (10+ call sites). Deleting `FsPath::parse` would require rewriting all resume handlers to extract path data from continuations instead. This is a valuable follow-up but out of scope for this task. The stripped `path.rs` imports types from `types.rs` and contains only the `FsPath` enum and `parse` method.

- [ ] **Step 3: Write the `#[omnifs::provider]` impl block**

Rewrite `lib.rs` to contain a single `#[omnifs::provider] impl GithubProvider` block with:
- `init`, `capabilities`, `resume` (from current `lib.rs` + `browse/mod.rs`)
- All `#[path]` handlers (logic moved from `browse/routing.rs`, one handler per path pattern)
- Helper methods without `#[path]` (like `namespace_handler`)

Route handlers use typed captures where applicable (`u64` for numeric IDs, `ResourceKind`/`StateFilter`/`RunFile`/`ResourceFile` for enum segments). `owner`/`repo` stay as `&str` with `is_safe_segment` guards.

**Handler ordering in the impl block (source order = priority):**

1. `#[path("/")]` root
2. `#[path("/{owner}")]` owner
3. `#[path("/{owner}/{repo}")]` repo
4. `#[path("/{owner}/{repo}/_issues")]`, `#[path("/{owner}/{repo}/_prs")]`, `#[path("/{owner}/{repo}/_actions")]`, `#[path("/{owner}/{repo}/_repo")]` (static 3-segment namespace handlers; each handles Lookup/List/Read for that namespace)
5. `#[path("/{owner}/{repo}/_repo/{*tree_path}")]` (rest capture, before dynamic 4-segment patterns)
6. `#[path("/{owner}/{repo}/_actions/runs")]`, `#[path("/{owner}/{repo}/_actions/runs/{run_id}")]`, `#[path("/{owner}/{repo}/_actions/runs/{run_id}/{file}")]` (action paths, before dynamic 4-segment patterns)
7. `#[path("/{owner}/{repo}/{ns}/{filter}")]` (dynamic, after all specific 3-and-4-segment patterns; `ns: ResourceKind` only matches `_issues`/`_prs`)
8. `#[path("/{owner}/{repo}/{ns}/{filter}/{number}")]` resource
9. `#[path("/{owner}/{repo}/{ns}/{filter}/{number}/comments")]` (literal before dynamic at depth 6)
10. `#[path("/{owner}/{repo}/{ns}/{filter}/{number}/comments/{idx}")]` comment file
11. `#[path("/{owner}/{repo}/{ns}/{filter}/{number}/{file}")]` resource file (dynamic catch-all at depth 6, after `comments` literal)

- [ ] **Step 4: Update `browse/mod.rs`**

Remove `mod routing;` and `pub use routing::{lookup_child, list_children, read_file};`. Keep everything else: `resume()`, `err`, `dispatch`, `dir_entry`, `file_entry`, `touch_repo`, `cache_only`, `extract_http_body`, `check_rate_limit`, `truncate_content`, and the submodule declarations (`mod events`, `mod files`, `mod resources`, `mod git`).

Note: many of these helpers are now also available from `omnifs_sdk`. The provider can gradually migrate to using SDK helpers and remove local duplicates. This is not required in this task; the goal is a working migration.

- [ ] **Step 5: Delete and strip old files**

- Delete `providers/github/src/browse/routing.rs`
- Strip `providers/github/src/path.rs`: remove all types that moved to `types.rs` (`Namespace`, `ResourceKind`, `StateFilter`, `ResourceFile`, `RunFile`, `is_safe_segment`, `is_numeric`, `is_safe_tree_path` and their impl blocks). Keep only `FsPath` enum + `FsPath::parse` + the `#[cfg(test)]` block. Update `path.rs` imports to use `crate::types::*`.
- Update `browse/files.rs`, `browse/resources.rs`, `browse/events.rs` imports from `crate::path::{ResourceKind, ...}` to `crate::types::{ResourceKind, ...}`. `FsPath` and `FsPath::parse` stay in `crate::path`.

Note: `FsPath::parse` survives this migration because resume handlers depend on it. A follow-up task should refactor resume handlers to extract path data from continuations, at which point `path.rs` can be fully deleted.

- [ ] **Step 6: Migrate tests**

Rewrite `FsPath::parse` tests as `__dispatch(Op::Lookup(1), "path")` assertions. Each former `FsPath::parse("some/path") == Some(FsPath::Variant { ... })` becomes a test that the dispatch returns an appropriate `ProviderResponse` (directory entry, file entry, or `None`).

- [ ] **Step 7: Verify**

Run: `cargo clippy -p omnifs-provider-github --target wasm32-wasip1 -- -D warnings && cargo test -p omnifs-provider-github --target wasm32-wasip1 --no-run`

- [ ] **Step 8: Commit**

```bash
git add providers/github/
git commit -m "refactor(github): adopt omnifs-sdk with typed path routing"
```

---

### Task 4: Migrate DNS provider

**Files:**
- Modify: `providers/dns/Cargo.toml`
- Rewrite: `providers/dns/src/lib.rs`
- Create: `providers/dns/src/types.rs`
- Delete: `providers/dns/src/path.rs`
- Delete: `providers/dns/src/browse/routing.rs`
- Modify: `providers/dns/src/browse/mod.rs`

- [ ] **Step 1: Update dependencies**

Same pattern as GitHub: add `omnifs-sdk`, remove `wit-bindgen` (WIT bindings come from SDK). Keep `strum` as a direct dependency (used by `RecordType`).

- [ ] **Step 2: Move `RecordType` to `types.rs`**

`RecordType` (with `strum` derives, `all()`, `common()`, `from_wire()`) moves from `path.rs` to `types.rs`. Update imports in `doh.rs`, `browse/mod.rs`, and `lib.rs`.

- [ ] **Step 3: Write the `#[omnifs::provider]` impl block**

DNS path patterns have runtime ambiguity (`@` prefix, IP vs domain at same depth). The handlers at `/{first}`, `/{first}/{second}`, `/{first}/{second}/{third}` use `&str` captures with runtime disambiguation in the handler body. This is the right tradeoff: templates show structural shape, handlers resolve semantic ambiguity.

`dispatch_batch` (needed for `_all` queries) is generated by the `#[omnifs::provider]` macro alongside `dispatch`. Handler bodies call `dispatch_batch(id, cont, effects)` directly; no import needed.

- [ ] **Step 4: Update `browse/mod.rs` and delete old files**

Same pattern as GitHub: remove `mod routing;` and routing re-exports, keep resume and helpers. Delete `path.rs` and `browse/routing.rs`.

- [ ] **Step 5: Migrate tests**

Same approach as GitHub.

- [ ] **Step 6: Verify**

Run: `cargo clippy -p omnifs-provider-dns --target wasm32-wasip1 -- -D warnings && cargo test -p omnifs-provider-dns --target wasm32-wasip1 --no-run`

- [ ] **Step 7: Commit**

```bash
git add providers/dns/
git commit -m "refactor(dns): adopt omnifs-sdk with typed path routing"
```

---

### Task 5: Migrate test provider

**Files:**
- Modify: `providers/test/Cargo.toml`
- Rewrite: `providers/test/src/lib.rs`

- [ ] **Step 1: Rewrite with SDK**

The test provider preserves its existing behavior (required by `crates/host/tests/runtime_test.rs` which expects `hello/message`, `hello/greeting`, and the KV effect/resume chain on `hello/cached`):

```rust
use omnifs_sdk::prelude::*;

#[derive(Deserialize)]
struct Config {}

struct State;

enum Continuation {
    AwaitingKvSet,
    AwaitingKvGet,
}

#[omnifs::provider]
impl TestProvider {
    fn init(_config: Config) -> (State, ProviderInfo) {
        (State, ProviderInfo {
            name: "test-provider".into(),
            version: "0.1.0".into(),
            description: "A test provider with canned data".into(),
        })
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["httpbin.org".into()],
            auth_types: vec![],
            max_memory_mb: 16,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 0,
        }
    }

    fn resume(id: u64, cont: Continuation, outcome: EffectResult) -> ProviderResponse {
        let result = match &outcome {
            EffectResult::Single(r) => r,
            EffectResult::Batch(v) if !v.is_empty() => &v[0],
            EffectResult::Batch(_) => return err("unexpected batch result"),
        };
        match cont {
            // After KV-set completes, issue a KV-get to read it back.
            Continuation::AwaitingKvSet => match result {
                SingleEffectResult::KvOk => dispatch(
                    id,
                    Continuation::AwaitingKvGet,
                    SingleEffect::KvGet("test:cached".into()),
                ),
                _ => err("expected KvOk"),
            },
            // After KV-get completes, return the value as file content.
            Continuation::AwaitingKvGet => match result {
                SingleEffectResult::KvValue(Some(data)) => {
                    ProviderResponse::Done(ActionResult::FileContent(data.clone()))
                }
                SingleEffectResult::KvValue(None) => {
                    ProviderResponse::Done(ActionResult::FileContent(b"no-cached-value".to_vec()))
                }
                _ => err("expected KvValue"),
            },
        }
    }

    #[path("/")]
    fn root(op: Op) -> Option<ProviderResponse> {
        match op {
            Op::List(_) => Some(ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                entries: vec![mk_dir("hello")],
                exhaustive: true,
            }))),
            _ => None,
        }
    }

    #[path("/hello")]
    fn hello(op: Op) -> Option<ProviderResponse> {
        match op {
            Op::Lookup(_) => Some(dir_entry("hello")),
            Op::List(_) => Some(ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                entries: vec![mk_file("message"), mk_file("greeting")],
                exhaustive: true,
            }))),
            _ => None,
        }
    }

    #[path("/hello/message")]
    fn message(op: Op) -> Option<ProviderResponse> {
        match op {
            Op::Lookup(_) => Some(file_entry("message")),
            Op::Read(_) => Some(ProviderResponse::Done(
                ActionResult::FileContent(b"Hello, world!".to_vec()),
            )),
            _ => None,
        }
    }

    #[path("/hello/greeting")]
    fn greeting(op: Op) -> Option<ProviderResponse> {
        match op {
            Op::Lookup(_) => Some(file_entry("greeting")),
            Op::Read(_) => Some(ProviderResponse::Done(
                ActionResult::FileContent(b"Hi there!\n".to_vec()),
            )),
            _ => None,
        }
    }

    #[path("/hello/cached")]
    fn cached(op: Op) -> Option<ProviderResponse> {
        match op {
            Op::Lookup(_) => Some(file_entry("cached")),
            // Triggers KV-set -> resume -> KV-get -> resume -> Done(FileContent)
            Op::Read(_) => Some(dispatch(
                op.id(),
                Continuation::AwaitingKvSet,
                SingleEffect::KvSet(KvSetRequest {
                    key: "test:cached".into(),
                    value: b"cached-value".to_vec(),
                }),
            )),
            _ => None,
        }
    }
}
```

- [ ] **Step 2: Verify**

Run: `cargo clippy -p test-provider --target wasm32-wasip1 -- -D warnings && cargo test -p test-provider --target wasm32-wasip1 --no-run`

- [ ] **Step 3: Commit**

```bash
git add providers/test/
git commit -m "refactor(test): adopt omnifs-sdk"
```

---

### Task 6: CI and build system updates

**Files:**
- Modify: `justfile`
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Update justfile**

Add the SDK crate to host-side test commands. Current `default-members` is `["crates/cli", "crates/host"]`, so `cargo test` does not include SDK tests:

```just
check: build-providers
    cargo fmt --all --check
    cargo clippy -- -D warnings
    cargo test -p omnifs-cli -p omnifs-host -p omnifs-sdk
    just check-providers
```

- [ ] **Step 2: Update CI**

In `rust-lint` job:
```yaml
- name: Run core clippy
  run: cargo clippy -p omnifs-cli -p omnifs-host -p omnifs-sdk -- -D warnings
```

In `rust-build-test` job:
```yaml
- name: Run core tests
  run: cargo nextest run --release -p omnifs-cli -p omnifs-host -p omnifs-sdk
```

- [ ] **Step 3: Migrate CI from `cargo component build` to two-step pipeline**

The CI `rust-build-test` job uses `cargo component build` to build providers. This contradicts CLAUDE.md which specifies a two-step pipeline (`cargo build --target wasm32-wasip1` + `wasm-tools component new`). The `cargo component build` step bundles a wasi adapter from v29, which is ABI-incompatible with wasmtime 43. This is a pre-existing issue, but the SDK migration is the right time to fix it.

Replace the CI provider build step with the same two-step pipeline from `justfile`'s `build-providers` recipe:

```yaml
- name: Install wasm-tools
  uses: taiki-e/install-action@v2
  with:
    tool: wasm-tools

- name: Build providers
  run: |
    set -euo pipefail
    adapter="build/wasi_snapshot_preview1.reactor.wasm"
    cargo build --target wasm32-wasip1 --release \
        -p omnifs-provider-github -p omnifs-provider-dns -p test-provider
    for wasm in target/wasm32-wasip1/release/omnifs_provider_*.wasm target/wasm32-wasip1/release/test_provider.wasm; do
        [ -f "$wasm" ] || continue
        wasm-tools component new "$wasm" \
            --adapt "wasi_snapshot_preview1=$adapter" \
            -o "$wasm"
    done
```

Remove the `cargo-component` install step from CI since it is no longer used.

- [ ] **Step 4: Verify full CI locally**

Run: `just check`

- [ ] **Step 5: Commit**

```bash
git add justfile .github/workflows/ci.yml
git commit -m "ci: add omnifs-sdk to build and test pipelines"
```

---

## Handler ordering reference

Handlers are tried in source order (top to bottom in the `#[omnifs::provider]` impl block). First match wins via the `.or_else()` chain.

**Ordering rules:**

1. **Root and static-prefix paths first:** `/`, `/_resolvers`, `/_reverse`, etc.
2. **Specific static namespaces before dynamic:** `/_issues`, `/_prs`, `/_actions`, `/_repo` before `/{ns}/{filter}`.
3. **Rest captures before same-depth dynamic:** `/_repo/{*tree_path}` before `/{ns}/{filter}` (both 4+ segments).
4. **Deeper static paths before shallower dynamic:** `/_actions/runs`, `/_actions/runs/{run_id}` before `/{ns}/{filter}`.
5. **Literal child before dynamic at same depth:** `.../comments` before `.../{file}`.
6. **Longer paths before shorter at same prefix:** `.../comments/{idx}` before `.../{file}`.

**General principle:** if two patterns could match the same path, the more constrained one must come first. Typed captures add implicit constraints (`u64` rejects non-numeric segments, `ResourceKind` rejects unknown namespaces) but ordering still matters when two patterns are structurally identical.

**Cross-capture constraints:** `FromStr` parses each capture independently. When validity depends on the combination of two captures (e.g., `ResourceFile::Diff` is only valid under `ResourceKind::Prs`, not `ResourceKind::Issues`), add explicit validation in the handler body:

```rust
#[path("/{owner}/{repo}/{ns}/{filter}/{number}/{file}")]
fn resource_file(op: Op, owner: &str, repo: &str, ns: ResourceKind, filter: StateFilter, number: u64, file: ResourceFile) -> Option<ProviderResponse> {
    if !is_safe_segment(owner) || !is_safe_segment(repo) { return None; }
    if file == ResourceFile::Diff && ns != ResourceKind::Prs { return None; }
    // ...
}
```

This is the standard pattern for constraints that cannot be expressed in the template syntax alone.

## Design notes

**Why `#[omnifs::provider]` on an impl block?** One annotation, one location. The macro sees lifecycle methods, route handlers, and helpers in a single impl block. No separate modules, no `routes = routes` parameter, no trait to implement. The macro infers Config, State, and Continuation types from method signatures.

**Why `FromStr` for typed captures?** It is the standard Rust trait for parsing strings into types. The macro generates `.parse::<T>().ok()?` for any capture parameter that is not `&str`. Failed parse returns `None`, causing fallthrough to the next route. This eliminates `is_numeric()`, `from_dir_name()`, and manual validation preambles. It also means adding a new validated type is just `impl FromStr for MyType`.

**Why `#[path("/")]` for root?** Consistent with the path template syntax. Every handler has a path template. `/` is the root path. The generated matcher checks `path.is_empty()`.

**Why an SDK crate instead of just a router?** The audit found ~100 lines of duplicated boilerplate per provider (state management, helpers, WIT trait stubs, TOML parsing). The SDK eliminates all of it. Providers contain only domain logic: config struct, state struct, continuation enum, route handlers, resume handlers.

**Proc macro crate runs on the host only.** `omnifs-sdk-macros` is a `proc-macro = true` crate using `syn`/`quote`. It runs at compile time and never touches wasm. The SDK runtime crate (`omnifs-sdk`) compiles to wasm32-wasip1 as a transitive dependency of providers.

## Migration gotchas

**WIT bindings live in the SDK.** The SDK runs `wit_bindgen::generate!` once and re-exports all types and traits. Providers remove their own `wit_bindgen::generate!` call and `bindings.rs`. All WIT types (`ProviderResponse`, `DirEntry`, etc.) and traits (`Guest`) are imported from `omnifs_sdk::`. This eliminates 34K lines of duplicated bindings per provider.

**`dispatch`/`dispatch_batch` are generated, not imported.** The `#[omnifs::provider]` macro generates these as `pub(crate)` functions alongside `with_state` and `with_pending`. They use `with_pending` to access the continuation map. The SDK does NOT export them. During migration, the existing `browse::dispatch` is updated to call `crate::with_pending(|p| p.insert(id, cont))` so browse submodules (`files.rs`, `resources.rs`) continue to work.

**`init` returns `(State, ProviderInfo)`.** The WIT `initialize` must return `ProviderInitialized(ProviderInfo)` with name, version, and description. The macro extracts the `ProviderInfo` from the tuple return and uses it in the generated response.

**GitHub `path.rs` is stripped, not deleted.** Resume submodules (`browse/files.rs`, `browse/resources.rs`, `browse/events.rs`) call `FsPath::parse` on stored path strings at 10+ call sites. The `FsPath` enum and `FsPath::parse` survive in a stripped `path.rs`. Types (`Namespace`, `ResourceKind`, `StateFilter`, `ResourceFile`, `RunFile`) and validators (`is_safe_segment`, `is_safe_tree_path`) move to `types.rs`. A follow-up task should refactor resume handlers to extract path data from continuations, at which point `path.rs` can be fully deleted.

**DNS `RecordType` needs its own module.** `RecordType` (with `strum` derives) currently lives in `path.rs`, which is deleted. It is used by `doh.rs`, `browse/mod.rs`, and `lib.rs`. Move it to `types.rs` and update imports. Keep `strum` as a direct dependency of the DNS provider (not in the SDK).

**Cross-capture validation for `ResourceFile::is_valid_for`.** `FromStr` parses each capture independently. `ResourceFile::Diff` is valid only for `ResourceKind::Prs`. Add `if file == ResourceFile::Diff && ns != ResourceKind::Prs { return None; }` in the `resource_file` handler body.

**Handlers no longer receive the raw FUSE `name` parameter.** The WIT `lookup_child(id, parent_path, name)` passes `name` separately. After migration, handlers extract it from path captures. For continuations that store `name` (like `ValidatingResource`), use the last capture variable or reconstruct from the path.

**`browse/mod.rs` is modified, not deleted.** Remove only `mod routing;` and `pub use routing::{lookup_child, list_children, read_file};`. Keep shared helpers and submodule declarations (`events`, `files`, `resources`, `git`). These are still needed by route handlers and the continuation state machine.

**GitHub `on_event` is non-trivial.** The GitHub provider dispatches `TimerTick` events to `browse::timer_tick(id)`. Include `fn on_event(id, event)` in the `#[omnifs::provider]` impl block. The macro delegates to it in `notify::Guest::on_event`.

**DNS `dispatch_batch` for `_all` queries.** The DNS `_all` record handler dispatches multiple effects in parallel. The macro generates `dispatch_batch` alongside `dispatch`.

**CI migrates from `cargo component build` to two-step pipeline.** Task 6 replaces the CI `cargo component build` step with the `cargo build --target wasm32-wasip1` + `wasm-tools component new` pipeline that `justfile` already uses locally. This fixes the pre-existing ABI mismatch with wasmtime 43.
