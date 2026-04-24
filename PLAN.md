# feat/migrate-crates-io

The crates-io provider in this worktree was built against the **old** provider SDK (`mounts!` + `Dir`/`Subtree` traits + `Projection<'_, P>` lifetime-parameterized + `Effect`/`ActionResult` terminal types + provider-side scope/identity/materialize semantics).

## Blocked by

This plan cannot start execution until both of these have merged into `main`:

- PR #28 `feat/sdk-http-post-support` — https://github.com/raulk/omnifs/pull/28
- PR #29 `feat/sdk-path-rest-captures` — https://github.com/raulk/omnifs/pull/29

Note: `ProviderError::rate_limited` / `::permission_denied` / `::version_mismatch`
constructors are already on `main` (landed with the #27 refactor). No separate PR
is needed.

## Execution model

This branch was created off `main` at `6343486`. To execute:

1. `git -C /Users/raul/W/gvfs worktree add /Users/raul/W/gvfs/.worktrees/migrate-crates-io feat/migrate-crates-io`
2. Work in that worktree only.
3. Bring in the provider source from the old worktree at
   `/Users/raul/W/gvfs/.worktrees/providers/crates-io/providers/crates-io/`
   per the "Port provider source" step below.
4. Execute this PLAN.md end-to-end. Corrections in the "Migration
   corrections" section are authoritative over anything in the reference
   body that contradicts them.
5. Run the Verification commands listed near the bottom.
6. Commit on the `feat/migrate-crates-io` branch, push, open PR.


## Port provider source

This branch is off `main` at `6343486`, so there is NO merge from
`wip/provider-crates-io-impl` and NO `git merge main`. The wip branch carries OLD-SDK infrastructure
that must not land here. Only provider-local files come over, file by file,
using `git checkout <old-branch> -- <path>` (this pulls the file contents into
the working tree and index without touching anything else).

### Files to copy verbatim (no touch-ups beyond rust import paths / `ProviderResult` → `Result`)

- `providers/crates-io/src/types.rs`
- `providers/crates-io/src/client.rs`

Bring each over with:

```bash
git checkout wip/provider-crates-io-impl -- providers/crates-io/src/types.rs
git checkout wip/provider-crates-io-impl -- providers/crates-io/src/client.rs
```

### Files to copy then touch up

_None._

### Files to create fresh (do NOT copy from the wip branch)

- `providers/crates-io/src/lib.rs`
- `providers/crates-io/src/provider.rs`
- `providers/crates-io/src/root.rs`
- `providers/crates-io/src/handlers/ (crate, version, owners, user tree)`

### Files to DISCARD (do NOT bring to this branch)

- `providers/crates-io/src/nodes.rs`
- `providers/crates-io/src/old provider.rs`
- `providers/crates-io/src/old lib.rs`

These are old-SDK artifacts (entity projections, tree walkers, routes tables,
manual http_ext wrappers for auth). The new SDK shape replaces them with
path-first handlers.

### Bring over the provider Cargo.toml

```bash
git checkout wip/provider-crates-io-impl -- providers/crates-io/Cargo.toml
```

Then update its SDK dependency declarations to match `providers/github/Cargo.toml`
on the current `main`. In particular, `omnifs-sdk` must point at the workspace
version and not an old path/git revision.

### Re-register the provider in the workspace

The workspace-level `Cargo.toml` on `main` dropped every non-dns/github/test
provider. Re-add `providers/crates-io` to its `members` array. Example diff:

```toml
[workspace]
members = [
    "crates/cli",
    "crates/host",
    "providers/dns",
    "providers/github",
+   "providers/crates-io",
    "providers/test",
]
```


## Migration corrections (authoritative)

The reference body below is the original `MIGRATION_PLAN.md` from the
`wip/provider-<name>-impl` worktree. It was written against the
provider-SDK worktrees' private snapshots and predates three base-SDK PRs
that have since landed on `main`. Where the reference body contradicts any
of the subsections here, THESE SUBSECTIONS WIN. Do not try to reconcile;
treat the reference body as historical context whose transport layer,
auth layer, and error layer have been updated.

### Auth handling

This provider works anonymously. Declare no auth requirement:

```rust
Capabilities {
    auth_types: vec![],
    domains: vec!["crates.io".to_string(), "static.crates.io".to_string(), "index.crates.io".to_string()],
    ..Default::default()
}
```

If an operator wants to raise rate limits by configuring a token on the
mount, the host can inject it transparently; the provider code does not
need to know. Do NOT add `token`, `api_key`, or similar fields to
`Config` or `State`. Do NOT add manual `Authorization` headers.

Domains covered:

  - `crates.io`
  - `static.crates.io`
  - `index.crates.io`

Mount config shape (anonymous is default; optional auth shown):

```json
{
  "plugin": "crates-io.wasm",
  "mount": "/crates-io",
  "auth": [{"type": "bearer-token", "token_env": "CRATES_IO_TOKEN", "domain": "crates.io"}]
}
```

### POST / JSON body shape (supersedes every manual Callout::Fetch/HttpRequest in the reference body)

PR #28 (`feat/sdk-http-post-support`) added first-class `post` / `body` /
`json` methods on the HTTP builder. Anywhere the reference body below
instructs the executor to assemble a raw `Callout::Fetch(HttpRequest {
method, headers, body, .. })`, use the builder surface instead:

```rust
let payload = serde_json::json!({
    "query": "query { viewer { id } }",
    "variables": {}
});

let bytes = cx.http()
    .post("https://api.example.com/graphql")
    .header("Accept", "application/json")
    .json(&payload)?            // Request::json returns Result<Self>; propagate with `?`
    .send_body()
    .await?;
```

Notes:

- `Request::json` auto-sets `Content-Type: application/json` unless the
  caller has already set one; don't double it up.
- For raw bytes (e.g. posting a tarball), use `.body(bytes)` and set
  `Content-Type` explicitly.
- Do NOT add an `Authorization` header here; the host injects it.
- Any reference-body passage that imports
  `omnifs_sdk::omnifs::provider::types::{Callout, Header, HttpRequest}` and
  manually constructs a `Callout::Fetch(HttpRequest { .. })` is superseded;
  use `cx.http().post(...)` / `.get(...)` instead.

A GET-with-headers equivalent:

```rust
let bytes = cx.http()
    .get(format!("{API_BASE}/v1/resource/{id}"))
    .header("Accept", "application/json")
    .send_body()
    .await?;
```

### Error constructors (supersedes remap workarounds in the reference body)

The `feat/sdk-error-constructors` branch (Track D) adds explicit
constructors on `ProviderError`. Use them directly; do NOT remap rate
limits / permission failures / version mismatches into
`ProviderError::invalid_input` with a string prefix.

```rust
use omnifs_sdk::error::ProviderError;

// 429
return Err(ProviderError::rate_limited(
    format!("upstream throttled: retry-after {retry_after}s")
));

// 401 / 403
return Err(ProviderError::permission_denied(
    "missing scope: repo:read".to_string()
));

// content-version mismatch
return Err(ProviderError::version_mismatch(
    format!("expected sha {expected}, got {actual}")
));
```

Any reference-body passage like "rate_limited maps to `invalid_input` with
a prefix" or "permission_denied is wrapped as `invalid_input`" is
superseded. Use the typed constructor.

## Destructive action approved

User approved the hard-reset path on 2026-04-24: the worktree's SDK work is
discarded; only provider-local files are salvaged.

Concretely, the old plan body calls for `git reset --hard` on the
`wip/provider-crates-io-impl` worktree. That instruction is OBSOLETE under
this new branching model. The new form:

- Do NOT merge `wip/provider-crates-io-impl` into this branch.
- Do NOT run any reset on the old worktree; leave it intact as a reference.
- Use `git checkout wip/provider-crates-io-impl -- providers/crates-io/<file>`
  in this worktree to salvage provider-local files one at a time, per the
  Port Provider Source list above.
- Nothing from `crates/omnifs-sdk*`, `crates/omnifs-mount-schema`,
  `crates/host`, `crates/cli`, or `wit/` comes over.

---

## Reference body (original MIGRATION_PLAN.md; subordinate to the corrections above)

> The content that follows was written for the old-SDK worktree at
> `/Users/raul/W/gvfs/.worktrees/providers/crates-io/MIGRATION_PLAN.md`.
> Read it for provider-specific shape, path tables, gotchas, and per-file
> migration notes. Wherever a passage conflicts with the corrections above
> (auth handling, POST shape, error constructors, rest captures, destructive
> action for crates-io), the corrections win.

# crates-io provider migration plan

Executable by sonnet end-to-end. Do not treat any step as optional without surfacing the deviation.

## Summary

The crates-io provider in this worktree was built against the **old** provider SDK (`mounts!` + `Dir`/`Subtree` traits + `Projection<'_, P>` lifetime-parameterized + `Effect`/`ActionResult` terminal types + provider-side scope/identity/materialize semantics). `main` has replaced the entire SDK, host runtime, and WIT with a **path-first** design built on free-function handlers, `Cx`-yielded async callouts, and a unified `ProviderReturn`.

The worktree's `crates/` and `wit/` trees diverged from main BEFORE main's redesign landed (fork point: `7742e99`; main tip: `6343486`; worktree tip: `e1d0b85`). A straight `git merge main` would produce deep conflicts across `crates/host/**`, `crates/omnifs-sdk*/**`, and `wit/provider.wit` (hundreds of files touched on both sides of the merge base, with the worktree's "mount-table + entity + materialize" design and main's "path-first + callouts" design being mutually incompatible redesigns of the same files).

Strategy: **replace** the worktree's `crates/`, `wit/`, `Cargo.lock`, and workspace `Cargo.toml` with main's contents (since there's no real shared work in those paths), then migrate the provider source in `providers/crates-io/src/**` by rewriting the three handler-carrying files to free-function handlers. Everything in `providers/crates-io/src/client.rs` and `providers/crates-io/src/types.rs` is preserved almost verbatim (only the API client's borrow of `Cx<State>` compiles unchanged, and `types.rs` has no SDK coupling).

Behavioral changes (flagged, not silently dropped):

- The old worktree exposed search under `/_search/{query}` as a dir that projected `results.json`. The new provider keeps that path shape; `results.json` becomes a projected file inside the dir.
- The old worktree modeled crate versions and owners as `Subtree` handlers on `/{krate}/versions` and `/{krate}/owners`. These were using the `Subtree` trait to drive arbitrary-depth tails through `lookup`/`list`/`read`. In the new SDK, `#[subtree]` is only for explicit git-backed tree handoff (returns a `tree-ref`). The closest mapping is a set of `#[dir]` and `#[file]` handlers with typed path captures: `/{krate}/versions`, `/{krate}/versions/{version}`, `/{krate}/versions/{version}/{field}`, `/{krate}/owners`, `/{krate}/owners/{login}`, `/{krate}/owners/{login}/{field}`. Functionally equivalent; implementation pattern matches github's `/{owner}/{repo}/_issues/...` ladder.
- Provider-side LRUs and TTLs: the old worktree had none already, but the new SDK forbids them outright. No-op for this provider.
- `Effect::CacheInvalidatePrefix` / `Identity` / `Scope`: the old worktree never emitted any. No-op for this provider. `on_event` is implemented as a no-op but wired so sonnet can extend it later without another migration.

## Current path table (verbatim from old `providers/crates-io/src/lib.rs`)

```rust
omnifs_sdk::mounts! {
    capture query: crate::types::SearchQuery;
    capture krate: crate::types::CrateName;

    "/" (dir) => Root;
    "/_search" (dir) => SearchRoot;
    "/_search/{query}" (dir) => SearchQueryDir;
    "/{krate}" (dir) => CrateDir;
    "/{krate}/versions" (subtree) => VersionsTree;
    "/{krate}/owners" (subtree) => OwnersTree;
}
```

Old shape of `VersionsTree` (via `RelPath` tail parsing inside the subtree):

```
/{krate}/versions                          (dir, list returns versions)
/{krate}/versions/{version}                (dir, list returns fields)
/{krate}/versions/{version}/{field}        (file, read returns bytes)
```

Old shape of `OwnersTree`:

```
/{krate}/owners                            (dir, list returns owners)
/{krate}/owners/{login}                    (dir, list returns fields)
/{krate}/owners/{login}/{field}            (file, read returns bytes)
```

## Target path table

Free-function handlers in `#[handlers] impl XxxHandlers` blocks registered on the provider via `#[provider(mounts(...))]`.

Root:

```
#[dir("/")]                                  Root listing (exhaustive, static children only)
#[dir("/_search")]                           Search root (dynamic under it; page-more)
#[dir("/_search/{query}")]                   Search results dir (projects results.json)
```

Crate:

```
#[dir("/{krate}")]                           Crate summary dir (projects crate fields)
```

Versions ladder (explodes the old `VersionsTree` Subtree into concrete dirs + files):

```
#[dir("/{krate}/versions")]                  Versions listing dir (children are version dirs)
#[dir("/{krate}/versions/{version}")]        Version fields dir (projects version fields)
#[file("/{krate}/versions/{version}/{field}")] Version field file
```

Owners ladder (explodes the old `OwnersTree` Subtree):

```
#[dir("/{krate}/owners")]                    Owners listing dir (children are owner dirs)
#[dir("/{krate}/owners/{login}")]            Owner fields dir
#[file("/{krate}/owners/{login}/{field}")]   Owner field file
```

No `#[subtree]` handlers: crates.io does not expose a git tree for crate versions, and the worktree's `VersionsTree`/`OwnersTree` weren't using `tree-ref` handoff; they were using the old trait's `lookup`/`list`/`read` methods (which are gone).

## SDK cheatsheet

Reference for rewriting every file. Copied from live SDK sources (`crates/omnifs-sdk/src/{prelude,handler,cx,http,git,error,init,browse}.rs`). Minor divergences from the source task's reference block are flagged inline.

### Provider registration (verbatim)

```rust
// lib.rs
pub(crate) use omnifs_sdk::prelude::Result;

mod client;
mod crate_dir;
mod owners;
mod provider;
mod root;
mod search;
pub(crate) mod types;
mod versions;

#[derive(Clone)]
pub(crate) struct State {
    pub config: Config,
}

#[derive(Clone)]
#[omnifs_sdk::config]
pub struct Config {
    #[serde(default = "default_api_base")]
    pub api_base: String,
    #[serde(default = "default_index_base")]
    pub index_base: String,
    #[serde(default = "default_search_page_size")]
    pub search_page_size: u32,
}

fn default_api_base() -> String { String::from("https://crates.io/api/v1") }
fn default_index_base() -> String { String::from("https://index.crates.io") }
fn default_search_page_size() -> u32 { 25 }
```

```rust
// provider.rs
use omnifs_sdk::Cx;
use omnifs_sdk::prelude::*;
use crate::{Config, State};

#[provider(mounts(
    crate::root::RootHandlers,
    crate::search::SearchHandlers,
    crate::crate_dir::CrateHandlers,
    crate::versions::VersionHandlers,
    crate::owners::OwnerHandlers,
))]
impl CratesIoProvider {
    fn init(config: Config) -> (State, ProviderInfo) {
        (
            State { config },
            ProviderInfo {
                name: "crates-io-provider".to_string(),
                version: "0.1.0".to_string(),
                description: "crates.io package metadata browsing".to_string(),
            },
        )
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["crates.io".to_string(), "index.crates.io".to_string()],
            auth_types: vec![],
            max_memory_mb: 64,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 0,
        }
    }

    async fn on_event(_cx: Cx<State>, _event: ProviderEvent) -> Result<EventOutcome> {
        // No event-driven invalidation yet. Stub kept so adding it later is a
        // one-file change, not a new WIT export to wire.
        Ok(EventOutcome::new())
    }
}
```

Notes:

- The `init` function may return either `(State, ProviderInfo)` or `Result<(State, ProviderInfo)>`. The macro detects both shapes (see `crates/omnifs-sdk-macros/src/provider_macro.rs::extract_init_types`).
- `on_event` is optional; declaring it keeps scaffolding in place even though the body is currently a no-op.

### Free-function handlers (verbatim pattern)

```rust
use omnifs_sdk::prelude::*;
use crate::{Result, State};

pub struct RootHandlers;

#[handlers]
impl RootHandlers {
    // sync dir
    #[dir("/")]
    fn root(_cx: &DirCx<'_, State>) -> Result<Projection> {
        let mut p = Projection::new();
        p.dir("_search");
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    // async dir with a typed capture
    #[dir("/_search/{query}")]
    async fn search(cx: &DirCx<'_, State>, query: crate::types::SearchQuery) -> Result<Projection> {
        let bytes = crate::client::CratesIoClient::new(cx)
            .search(&query)
            .await
            .and_then(|view| crate::client::to_pretty_json_bytes(&view))?;
        let mut p = Projection::new();
        p.file_with_content("results.json", bytes);
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    // file (exact path)
    #[file("/{krate}/versions/{version}/{field}")]
    async fn version_field(
        cx: &Cx<State>,
        krate: crate::types::CrateName,
        version: crate::types::Version,
        field: crate::types::VersionField,
    ) -> Result<FileContent> {
        /* ... */
        Ok(FileContent::bytes(b"".to_vec()))
    }
}
```

Rules:

- Path captures (`{...}`) become positional args. Any type implementing `FromStr` works. The order of args must match the order the captures appear in the template.
- `DirCx<'_, S>` derefs to `Cx<S>`; call `.http()`, `.git()`, `.state()` directly.
- `DirIntent` is accessed via `cx.intent()` in a dir handler and lets you branch between `Lookup { child }`, `List { cursor }`, and `ReadProjectedFile { name }` inside one handler (used by github's `comments_projection`). For simple "always return the full dir" handlers, ignore `intent()`.
- Handlers may be sync or `async fn`.
- `Projection::new()` + `.dir(name)` / `.file(name)` / `.file_with_stat(name, stat)` / `.file_with_content(name, bytes)`. Eager content must be `<= 64 KiB` (`MAX_PROJECTED_BYTES` in `handler.rs`); larger writes land in `projection.error` and the projection becomes `Err(InvalidInput)` on dispatch.
- Pagination: `p.page(PageStatus::More(Cursor::Opaque("cursor".into())))` or `p.page(PageStatus::Exhaustive)`. If `page` is unset, the host treats the listing as exhaustive.
- Preload cache: `p.preload(path, bytes)` / `p.preload_many(iter)` to flow content the host should cache alongside the listing.

### Context `Cx<S>`

- `cx.state(|s| ...)` / `cx.state_mut(|s| ...)`: `FnOnce` closures, no lifetime leakage. Return any owned value including `Result<T>`.
- `cx.http()` builder: `.get(url)`. `Request::header(name, value)`, `Request::send_body().await -> Result<Vec<u8>>`, `Request::send().await -> Result<HttpResponse>`. No `.post(url)` in the current SDK (add locally if needed; currently crates-io provider only reads). No `.json(&body)` helper in `http.rs` either, it's a direct body setter when needed; crates-io only uses GET so this is n/a.
- `cx.git()`: `.open_repo(cache_key, clone_url).await -> Result<GitRepoInfo { repo, tree }>`. Not used by crates-io. **Note:** the task brief's reference shows `cx.git().open(url)` returning `tree_ref`; the actual SDK method is `open_repo(cache_key, clone_url)` and the field is `tree`. Use the actual SDK shape.
- `join_all(futs)` for parallel callouts. Every child future must yield exactly one callout per suspension and share the same `Cx`.

### Caching, errors, browse terminals

- Host owns caching; no provider LRUs or TTLs.
- File sizes must be non-zero (the SDK's `Projection::file()` auto-fills a 4096-byte placeholder; `file_with_stat` takes a `NonZeroU64`; `file_with_content(name, bytes)` sizes from `bytes.len()` automatically).
- Sibling/preload: `Projection::preload`, `Lookup::with_sibling_files`, `FileContent::with_sibling_files`.
- Invalidation: host-side. Express via `EventOutcome::invalidate_path`/`invalidate_prefix` in `on_event`. Scope and identity invalidation are gone.
- Errors: `ProviderError::{not_found, invalid_input, internal, not_a_directory, not_a_file, unimplemented, permission_denied, network, timeout, denied, too_large, rate_limited, version_mismatch}`.

## Bring worktree up to main

The worktree's `wip/provider-crates-io-impl` branch is based on `7742e99` and has its own redesign of `crates/` and `wit/` (mount-table + entity + materialize). Main's `6343486` redesigned the same files in a different direction (path-first + callouts). A literal `git merge main` or `git rebase main` conflicts across ~100 files with no meaningful overlap worth preserving, so **discard the worktree's versions of those files and take main's wholesale**. The worktree-specific commits that are genuinely worth preserving are limited to:

1. The provider crate itself under `providers/crates-io/` (currently untracked per `git status`, but present on disk; we are rewriting its source anyway).
2. The `docs/provider-design-crates-io.md` design doc (untracked; keep as-is; no code impact).
3. The workspace `Cargo.toml` addition of `providers/crates-io` to `members` (trivial; will be re-added).

### Step 1: start from a clean, up-to-date base

```bash
cd /Users/raul/W/gvfs/.worktrees/providers/crates-io

# Save the provider design doc and provider crate out-of-tree (belt and braces;
# they are already untracked so a reset won't touch them, but a copy makes the
# rest of the plan easier to reason about).
mkdir -p /tmp/crates-io-migration-save
cp -R providers/crates-io /tmp/crates-io-migration-save/
cp docs/provider-design-crates-io.md /tmp/crates-io-migration-save/

# Fetch main and hard-reset the working branch to main. This discards the
# worktree's entire SDK/host redesign (which is superseded by main's) and the
# CLI/Dockerfile/CI edits from the worktree (which will not apply on main's
# layout).
git fetch origin main:main 2>/dev/null || git fetch --all
git reset --hard main

# Re-create the working branch pointing at main (preserve the existing branch
# name for continuity).
git checkout -B wip/provider-crates-io-impl
```

Rationale for hard-reset vs merge: the worktree's commits between the fork point and HEAD touch the same subsystems main redesigned. None of those commits land cleanly on top of main; all of them are superseded. The only commit payload worth keeping is the provider crate, which we're rewriting anyway.

### Step 2: restore provider files under the new layout

```bash
mkdir -p providers/crates-io/src
cp /tmp/crates-io-migration-save/crates-io/Cargo.toml       providers/crates-io/Cargo.toml
cp /tmp/crates-io-migration-save/crates-io/src/client.rs    providers/crates-io/src/client.rs
cp /tmp/crates-io-migration-save/crates-io/src/types.rs     providers/crates-io/src/types.rs
# lib.rs, provider.rs, nodes.rs will be rewritten by step 4; do NOT copy.
cp /tmp/crates-io-migration-save/provider-design-crates-io.md docs/provider-design-crates-io.md
```

### Step 3: wire the provider into the workspace

Edit `/Users/raul/W/gvfs/.worktrees/providers/crates-io/Cargo.toml` (root workspace):

```toml
[workspace]
resolver = "2"
members = [
    "crates/*",
    "providers/github",
    "providers/dns",
    "providers/crates-io",
    "providers/test",
]
default-members = ["crates/cli", "crates/host"]
```

### Step 4: rewrite the provider source

See "Per-file migration" below for exact contents of each file.

### Step 5: verify

See "Verification checklist" below.

### Conflict resolution guidance (only if you choose to merge instead)

If you must preserve the worktree's existing SDK work for some reason (you should not; it's all superseded), resolve merge conflicts by taking **main's version wholesale** for:

- `wit/provider.wit`
- `crates/omnifs-sdk/**`
- `crates/omnifs-sdk-macros/**`
- `crates/omnifs-mount-schema/**`
- `crates/host/**`
- `crates/cli/**`
- `Dockerfile`, `.github/workflows/ci.yml`, `scripts/container-entrypoint.sh`, `justfile`

And re-apply by hand: the `providers/crates-io` member in the workspace Cargo.toml. Every other worktree change is either superseded or a stale bug-for-bug port of the old SDK.

## Per-file migration

Relative paths are from `/Users/raul/W/gvfs/.worktrees/providers/crates-io/`.

### `providers/crates-io/Cargo.toml` — **KEEP** (minor edits)

Current content compiles against the new SDK (it depends only on `omnifs-sdk` + serde). The `[package.metadata.component]` block is vestigial per the project CLAUDE.md but harmless. Keep as-is. Add `hashbrown` for map internals consistency with other providers (not strictly required because this provider does not keep maps in State, but if you add etag-style state later you will need it; leave out for now).

Final Cargo.toml for the provider (unchanged from the worktree's current version):

```toml
[package]
name = "omnifs-provider-crates-io"
version = "0.1.0"
edition = "2024"
description = "OmnIFS provider for crates.io metadata browsing"
license = "MIT OR Apache-2.0"
repository = "https://github.com/raulk/omnifs"
homepage = "https://github.com/raulk/omnifs"

[lib]
crate-type = ["cdylib", "lib"]

[dependencies]
omnifs-sdk = { path = "../../crates/omnifs-sdk" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[package.metadata.component]
package = "omnifs:provider"

[package.metadata.component.target]
world = "provider"
path = "../../wit"

[lints.rust]
unsafe_code = "allow"

[lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"
must_use_candidate = "allow"
wildcard_imports = "allow"
too_many_lines = "allow"
trivially_copy_pass_by_ref = "allow"
match_same_arms = "allow"
needless_pass_by_value = "allow"
unnecessary_wraps = "allow"
```

### `providers/crates-io/src/types.rs` — **KEEP VERBATIM**

No SDK coupling. `CrateName`, `SearchQuery`, `OwnerLogin`, `is_safe_nonempty_segment`, and `percent_encode_component` all remain usable. `FromStr` impls still satisfy the handler-capture contract. Add one new type to front the `versions/{version}` capture (a thin wrapper around `String` that validates path-safety); this keeps version fields consistently typed across the ladder. Add another for the `{field}` capture at the leaves (a closed-enum wrapper ensures invalid fields 404 at parse time instead of at the inner match in `version_field_bytes`).

Append these two types to the bottom of `types.rs` (before the `#[cfg(test)]` block):

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Version(String);

impl FromStr for Version {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !is_safe_nonempty_segment(s) {
            return Err(());
        }
        Ok(Self(s.to_string()))
    }
}

impl AsRef<str> for Version {
    fn as_ref(&self) -> &str { &self.0 }
}

impl Display for Version {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum VersionField {
    Description,
    License,
    RustVersion,
    Edition,
    CrateSize,
    Downloads,
    CreatedAt,
    UpdatedAt,
    Yanked,
    YankMessage,
    Checksum,
    ReadmePath,
    DownloadUrl,
    FeaturesJson,
    DependenciesJson,
}

impl FromStr for VersionField {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "description" => Self::Description,
            "license" => Self::License,
            "rust_version" => Self::RustVersion,
            "edition" => Self::Edition,
            "crate_size" => Self::CrateSize,
            "downloads" => Self::Downloads,
            "created_at" => Self::CreatedAt,
            "updated_at" => Self::UpdatedAt,
            "yanked" => Self::Yanked,
            "yank_message" => Self::YankMessage,
            "checksum" => Self::Checksum,
            "readme_path" => Self::ReadmePath,
            "download_url" => Self::DownloadUrl,
            "features.json" => Self::FeaturesJson,
            "dependencies.json" => Self::DependenciesJson,
            _ => return Err(()),
        })
    }
}

impl Display for VersionField {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Description => "description",
            Self::License => "license",
            Self::RustVersion => "rust_version",
            Self::Edition => "edition",
            Self::CrateSize => "crate_size",
            Self::Downloads => "downloads",
            Self::CreatedAt => "created_at",
            Self::UpdatedAt => "updated_at",
            Self::Yanked => "yanked",
            Self::YankMessage => "yank_message",
            Self::Checksum => "checksum",
            Self::ReadmePath => "readme_path",
            Self::DownloadUrl => "download_url",
            Self::FeaturesJson => "features.json",
            Self::DependenciesJson => "dependencies.json",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum OwnerField {
    Login,
    Name,
}

impl FromStr for OwnerField {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "login" => Self::Login,
            "name" => Self::Name,
            _ => return Err(()),
        })
    }
}

impl Display for OwnerField {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Login => "login",
            Self::Name => "name",
        })
    }
}
```

### `providers/crates-io/src/client.rs` — **KEEP** (minimal change)

The client's public surface (`CratesIoClient::new(cx)`, `search`, `crate_summary`, `sparse_versions`, `version_record`, `owners`, `parse_json`, `to_pretty_json_bytes`) is unchanged. The only touch-up needed:

- The old file's single import line `use omnifs_sdk::prelude::ProviderError;` remains valid. `use omnifs_sdk::Cx;` is also valid (re-exported from the crate root). `use omnifs_sdk::http::Request;` is valid. No edits are required.

Keep the file as-is. If sonnet observes any compilation diagnostic from this file, the fix is almost certainly in `ProviderResult<T>`'s definition (moved to `lib.rs`). Verify `pub(crate) type ProviderResult<T> = std::result::Result<T, ProviderError>;` is still declared in `lib.rs` (it is below).

### `providers/crates-io/src/lib.rs` — **REWRITE** (full content)

```rust
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use omnifs_sdk::prelude::ProviderError;

mod client;
mod crate_dir;
mod owners;
mod provider;
mod root;
mod search;
pub(crate) mod types;
mod versions;

#[derive(Clone)]
pub(crate) struct State {
    pub config: Config,
}

#[derive(Clone)]
#[omnifs_sdk::config]
pub struct Config {
    #[serde(default = "default_api_base")]
    pub api_base: String,
    #[serde(default = "default_index_base")]
    pub index_base: String,
    #[serde(default = "default_search_page_size")]
    pub search_page_size: u32,
}

fn default_api_base() -> String { String::from("https://crates.io/api/v1") }
fn default_index_base() -> String { String::from("https://index.crates.io") }
fn default_search_page_size() -> u32 { 25 }

pub(crate) type ProviderResult<T> = std::result::Result<T, ProviderError>;
pub(crate) use omnifs_sdk::prelude::Result;
```

Notes:

- The old `mounts! { ... }` block is replaced by the `#[provider(mounts(...))]` attribute in `provider.rs`.
- `pub(crate) use omnifs_sdk::prelude::Result` is added because the handler modules use `Result<T>` (the SDK alias) and `client.rs` uses `ProviderResult<T>`. Both aliases are the same type; both coexist for minimal churn in `client.rs`.

### `providers/crates-io/src/provider.rs` — **REWRITE** (full content)

```rust
use omnifs_sdk::Cx;
use omnifs_sdk::prelude::*;

use crate::{Config, State};

#[provider(mounts(
    crate::root::RootHandlers,
    crate::search::SearchHandlers,
    crate::crate_dir::CrateHandlers,
    crate::versions::VersionHandlers,
    crate::owners::OwnerHandlers,
))]
impl CratesIoProvider {
    fn init(config: Config) -> (State, ProviderInfo) {
        (
            State { config },
            ProviderInfo {
                name: "crates-io-provider".to_string(),
                version: "0.1.0".to_string(),
                description: "crates.io package metadata browsing".to_string(),
            },
        )
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["crates.io".to_string(), "index.crates.io".to_string()],
            auth_types: vec![],
            max_memory_mb: 64,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 0,
        }
    }

    async fn on_event(_cx: Cx<State>, _event: ProviderEvent) -> Result<EventOutcome> {
        Ok(EventOutcome::new())
    }
}
```

### `providers/crates-io/src/root.rs` — **NEW** (replaces old `Root`/`SearchRoot` in `nodes.rs`)

```rust
use omnifs_sdk::prelude::*;

use crate::{Result, State};

pub struct RootHandlers;

#[handlers]
impl RootHandlers {
    #[dir("/")]
    fn root(_cx: &DirCx<'_, State>) -> Result<Projection> {
        // The static child list (`_search`, plus any `/{krate}` dir the user
        // walks into) is derived from the #[dir] / #[file] templates on
        // RootHandlers, SearchHandlers, and CrateHandlers. Nothing to project
        // dynamically: the `/_search` directory name is auto-derived from
        // SearchHandlers' `#[dir("/_search")]`, and `/{krate}` is captured by
        // CrateHandlers and not enumerable (crates.io has no "all crates"
        // listing).
        let mut projection = Projection::new();
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }
}
```

### `providers/crates-io/src/search.rs` — **NEW** (replaces old `SearchRoot`/`SearchQueryDir`)

```rust
use omnifs_sdk::prelude::*;

use crate::client::{CratesIoClient, to_pretty_json_bytes};
use crate::types::SearchQuery;
use crate::{Result, State};

pub struct SearchHandlers;

#[handlers]
impl SearchHandlers {
    #[dir("/_search")]
    fn search_root(_cx: &DirCx<'_, State>) -> Result<Projection> {
        // Search root is not enumerable (crates.io has no "all queries" list).
        // Users navigate by path: /_search/{encoded-query}.
        let mut projection = Projection::new();
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[dir("/_search/{query}")]
    async fn search_query(cx: &DirCx<'_, State>, query: SearchQuery) -> Result<Projection> {
        let results = CratesIoClient::new(cx).search(&query).await?;
        let bytes = to_pretty_json_bytes(&results)?;
        let mut projection = Projection::new();
        projection.file_with_content("results.json", bytes);
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }
}
```

### `providers/crates-io/src/crate_dir.rs` — **NEW** (replaces old `CrateDir`)

```rust
use omnifs_sdk::prelude::*;

use crate::client::CratesIoClient;
use crate::types::CrateName;
use crate::{Result, State};

pub struct CrateHandlers;

#[handlers]
impl CrateHandlers {
    #[dir("/{krate}")]
    async fn crate_dir(cx: &DirCx<'_, State>, krate: CrateName) -> Result<Projection> {
        let record = CratesIoClient::new(cx).crate_summary(&krate).await?;

        let mut projection = Projection::new();
        project_optional_file(&mut projection, "description", record.description.as_deref());
        project_optional_file(&mut projection, "homepage", record.homepage.as_deref());
        project_optional_file(&mut projection, "repository", record.repository.as_deref());
        project_optional_file(&mut projection, "documentation", record.documentation.as_deref());
        projection.file_with_content("downloads", record.downloads.to_string().into_bytes());
        if let Some(recent) = record.recent_downloads {
            projection.file_with_content("recent_downloads", recent.to_string().into_bytes());
        }
        projection.file_with_content("max_version", record.max_version.into_bytes());
        if let Some(max_stable) = record.max_stable_version {
            projection.file_with_content("max_stable_version", max_stable.into_bytes());
        }
        projection.file_with_content("newest_version", record.newest_version.into_bytes());
        projection.file_with_content("default_version", record.default_version.into_bytes());
        projection.file_with_content("created_at", record.created_at.into_bytes());
        projection.file_with_content("updated_at", record.updated_at.into_bytes());
        projection.file_with_content(
            "yanked",
            if record.yanked { b"true".to_vec() } else { b"false".to_vec() },
        );
        projection.file_with_content("keywords.json", record.keywords_json);
        projection.file_with_content("categories.json", record.categories_json);

        // `versions/` and `owners/` dirs are static children auto-derived from
        // VersionHandlers' `#[dir("/{krate}/versions")]` and OwnerHandlers'
        // `#[dir("/{krate}/owners")]`; the SDK merges them into this listing.

        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }
}

fn project_optional_file(projection: &mut Projection, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        projection.file_with_content(name, value.as_bytes().to_vec());
    }
}
```

### `providers/crates-io/src/versions.rs` — **NEW** (replaces old `VersionsTree` subtree)

```rust
use omnifs_sdk::prelude::*;

use crate::client::{CratesIoClient, VersionRecord};
use crate::types::{CrateName, Version, VersionField};
use crate::{Result, State};

pub struct VersionHandlers;

#[handlers]
impl VersionHandlers {
    #[dir("/{krate}/versions")]
    async fn versions(cx: &DirCx<'_, State>, krate: CrateName) -> Result<Projection> {
        let versions = CratesIoClient::new(cx).sparse_versions(&krate).await?;
        let mut projection = Projection::new();
        // Newest first: matches the old VersionsTree, which reversed
        // sparse order for display.
        for record in versions.into_iter().rev() {
            projection.dir(record.num);
        }
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[dir("/{krate}/versions/{version}")]
    async fn version_dir(
        cx: &DirCx<'_, State>,
        krate: CrateName,
        version: Version,
    ) -> Result<Projection> {
        let record = CratesIoClient::new(cx)
            .version_record(&krate, version.as_ref())
            .await?;
        let mut projection = Projection::new();
        project_version_fields(&mut projection, &record);
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[file("/{krate}/versions/{version}/{field}")]
    async fn version_field(
        cx: &Cx<State>,
        krate: CrateName,
        version: Version,
        field: VersionField,
    ) -> Result<FileContent> {
        let record = CratesIoClient::new(cx)
            .version_record(&krate, version.as_ref())
            .await?;
        let bytes = version_field_bytes(&record, field).ok_or_else(|| {
            ProviderError::not_found(format!("version field not found: {field}"))
        })?;
        Ok(FileContent::bytes(bytes))
    }
}

fn project_version_fields(projection: &mut Projection, record: &VersionRecord) {
    if let Some(description) = &record.description {
        projection.file_with_content("description", description.clone().into_bytes());
    }
    if let Some(license) = &record.license {
        projection.file_with_content("license", license.clone().into_bytes());
    }
    if let Some(rust_version) = &record.rust_version {
        projection.file_with_content("rust_version", rust_version.clone().into_bytes());
    }
    if let Some(edition) = &record.edition {
        projection.file_with_content("edition", edition.clone().into_bytes());
    }
    projection.file_with_content("crate_size", record.crate_size.to_string().into_bytes());
    projection.file_with_content("downloads", record.downloads.to_string().into_bytes());
    if let Some(created_at) = &record.created_at {
        projection.file_with_content("created_at", created_at.clone().into_bytes());
    }
    if let Some(updated_at) = &record.updated_at {
        projection.file_with_content("updated_at", updated_at.clone().into_bytes());
    }
    projection.file_with_content(
        "yanked",
        if record.yanked { b"true".to_vec() } else { b"false".to_vec() },
    );
    if let Some(yank_message) = &record.yank_message {
        projection.file_with_content("yank_message", yank_message.clone().into_bytes());
    }
    projection.file_with_content("checksum", record.checksum.clone().into_bytes());
    if let Some(readme_path) = &record.readme_path {
        projection.file_with_content("readme_path", readme_path.clone().into_bytes());
    }
    projection.file_with_content("download_url", record.download_url.clone().into_bytes());
    projection.file_with_content("features.json", record.features_json.clone());
    projection.file_with_content("dependencies.json", record.dependencies_json.clone());
}

fn version_field_bytes(record: &VersionRecord, field: VersionField) -> Option<Vec<u8>> {
    match field {
        VersionField::Description => record.description.clone().map(String::into_bytes),
        VersionField::License => record.license.clone().map(String::into_bytes),
        VersionField::RustVersion => record.rust_version.clone().map(String::into_bytes),
        VersionField::Edition => record.edition.clone().map(String::into_bytes),
        VersionField::CrateSize => Some(record.crate_size.to_string().into_bytes()),
        VersionField::Downloads => Some(record.downloads.to_string().into_bytes()),
        VersionField::CreatedAt => record.created_at.clone().map(String::into_bytes),
        VersionField::UpdatedAt => record.updated_at.clone().map(String::into_bytes),
        VersionField::Yanked => Some(if record.yanked {
            b"true".to_vec()
        } else {
            b"false".to_vec()
        }),
        VersionField::YankMessage => record.yank_message.clone().map(String::into_bytes),
        VersionField::Checksum => Some(record.checksum.clone().into_bytes()),
        VersionField::ReadmePath => record.readme_path.clone().map(String::into_bytes),
        VersionField::DownloadUrl => Some(record.download_url.clone().into_bytes()),
        VersionField::FeaturesJson => Some(record.features_json.clone()),
        VersionField::DependenciesJson => Some(record.dependencies_json.clone()),
    }
}
```

Implementation observations:

- The `version_dir` handler projects every field as an eager file. This doubles as the dir listing AND serves each field's content directly (the host reads projected bytes without a second provider call). This matches the preload/sibling idiom the project CLAUDE.md asks providers to use whenever the payload already carries the fields.
- The `version_field` file handler remains available for direct path reads (`cat /{krate}/versions/1.2.3/downloads`). The host prefers exact `#[file]` handlers over projected bytes from `#[dir]`, which matches the browse dispatch order documented in the host runtime.
- If a reader is concerned about duplicate API fetches between listing and reading: the host caches the listing AND the projected file content; the `version_field` handler is the fallback path the host takes only when the dir listing's preload has been evicted. A second round-trip is the correct failure mode (host-owned caching, no provider LRUs).

### `providers/crates-io/src/owners.rs` — **NEW** (replaces old `OwnersTree` subtree)

```rust
use omnifs_sdk::prelude::*;

use crate::client::{CratesIoClient, OwnerRecord};
use crate::types::{CrateName, OwnerField, OwnerLogin};
use crate::{Result, State};

pub struct OwnerHandlers;

#[handlers]
impl OwnerHandlers {
    #[dir("/{krate}/owners")]
    async fn owners(cx: &DirCx<'_, State>, krate: CrateName) -> Result<Projection> {
        let owners = CratesIoClient::new(cx).owners(&krate).await?;
        let mut projection = Projection::new();
        for owner in owners {
            projection.dir(owner.login.to_string());
        }
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[dir("/{krate}/owners/{login}")]
    async fn owner_dir(
        cx: &DirCx<'_, State>,
        krate: CrateName,
        login: OwnerLogin,
    ) -> Result<Projection> {
        let owner = find_owner(cx, &krate, &login).await?;
        let mut projection = Projection::new();
        projection.file_with_content("login", owner.login.as_ref().as_bytes().to_vec());
        if let Some(name) = &owner.name {
            projection.file_with_content("name", name.clone().into_bytes());
        }
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[file("/{krate}/owners/{login}/{field}")]
    async fn owner_field(
        cx: &Cx<State>,
        krate: CrateName,
        login: OwnerLogin,
        field: OwnerField,
    ) -> Result<FileContent> {
        let owner = find_owner(cx, &krate, &login).await?;
        let bytes = match field {
            OwnerField::Login => owner.login.as_ref().as_bytes().to_vec(),
            OwnerField::Name => owner
                .name
                .ok_or_else(|| ProviderError::not_found("owner name not set"))?
                .into_bytes(),
        };
        Ok(FileContent::bytes(bytes))
    }
}

async fn find_owner(
    cx: &Cx<State>,
    krate: &CrateName,
    login: &OwnerLogin,
) -> Result<OwnerRecord> {
    CratesIoClient::new(cx)
        .owners(krate)
        .await?
        .into_iter()
        .find(|owner| owner.login.as_ref() == login.as_ref())
        .ok_or_else(|| ProviderError::not_found(format!("owner not found: {login}")))
}
```

Note the capture type `login: OwnerLogin`: `OwnerLogin::from_str` already rejects path-unsafe segments in `types.rs`, so malformed lookups 404 at parse time.

### `providers/crates-io/src/nodes.rs` — **DELETE**

All content is superseded by `root.rs`, `search.rs`, `crate_dir.rs`, `versions.rs`, and `owners.rs`. The helpers `parse_versions_tail`/`parse_owners_tail`/`version_field_bytes`/`owner_field_bytes`/`project_optional_file` are replaced by the new capture types and the inline projection helpers inside each handler module. Tests inside the old module are replaced as a group by the new module-level tests below.

## Event handling migration

The old worktree has **zero** `Effect::CacheInvalidate{Prefix,Identity,Scope}` emissions (search for "CacheInvalidate" in `/Users/raul/W/gvfs/.worktrees/providers/crates-io/providers/crates-io/src/**` returns nothing). There is no behavior to carry forward.

The `on_event(_cx, _event) -> Result<EventOutcome>` stub in `provider.rs` is in place so sonnet can add invalidation later without re-wiring the WIT export. If crate version cache invalidation becomes desirable (e.g. TimerTick firing a crates.io API call to learn about new versions), the pattern to follow is `providers/github/src/events.rs::timer_tick`: pull active paths via `cx.active_paths(mount_id, parse_fn)`, fetch per-crate diffs, then emit `outcome.invalidate_prefix(format!("{krate}/versions"))`.

## Cargo.toml changes

### Workspace root `Cargo.toml`

Replace the `members` array. Final content:

```toml
[workspace]
resolver = "2"
members = [
    "crates/*",
    "providers/github",
    "providers/dns",
    "providers/crates-io",
    "providers/test",
]
default-members = ["crates/cli", "crates/host"]

[workspace.dependencies]
wasmtime = { version = "43", features = ["component-model", "runtime"] }
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json", "stream", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
schemars = "1"
jsonschema = "0.46"
postcard = { version = "1", features = ["alloc"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
thiserror = "2"
clap = { version = "4", features = ["derive"] }
dashmap = "6"
parking_lot = "0.12"
tempfile = "3"
libc = "0.2"
anyhow = "1"
redb = "2"
moka = { version = "0.12", features = ["sync"] }

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"
must_use_candidate = "allow"

[workspace.lints.rust]
unsafe_code = "warn"
```

### Provider `providers/crates-io/Cargo.toml`

No changes. Verbatim as listed under "Per-file migration" above.

## Verification checklist

Run in order. Every command must succeed with zero warnings under `-D warnings`.

```bash
cd /Users/raul/W/gvfs/.worktrees/providers/crates-io

# 1. Workspace-wide format.
cargo fmt --check

# 2. Host-side checks (CLI, host, SDK, macros).
cargo clippy --workspace --exclude omnifs-provider-crates-io --exclude omnifs-provider-github --exclude omnifs-provider-dns --exclude test-provider -- -D warnings
cargo test --workspace --exclude omnifs-provider-crates-io --exclude omnifs-provider-github --exclude omnifs-provider-dns --exclude test-provider

# 3. Provider compile + lint + test-compile under the wasm target.
cargo clippy -p omnifs-provider-crates-io --target wasm32-wasip2 -- -D warnings
cargo test   -p omnifs-provider-crates-io --target wasm32-wasip2 --no-run

# 4. All-provider sweep (same as step 3 for github and dns; test-provider is a stub).
just check-providers
```

Expected outcomes:

- `cargo fmt --check` exits 0.
- `cargo clippy ... -D warnings` exits 0 with no warnings.
- `cargo test --no-run` compiles the provider's `#[cfg(test)]` modules on the wasm32 target. Tests are not executed (the project CLAUDE.md notes "Provider tests can't execute on wasm32-wasip2 directly in Cargo's test harness"). The old worktree's tests in `nodes.rs` test tail parsing that no longer exists; drop them. The old tests in `types.rs` (`crate_name_normalizes_to_sparse_index_rules`, `search_query_decodes_and_reencodes_segments`, `percent_encoder_escapes_reserved_bytes`) remain valid; keep them. The old tests in `client.rs` (`parse_sparse_index_line_handles_optional_fields`, `pretty_json_serializer_is_stable_for_dependency_views`, `parse_json_rejects_invalid_payloads`) remain valid; keep them.
- `just check-providers` exits 0.

If clippy raises `clippy::too_many_arguments` on any handler, that is expected for the 4-arg versions handlers (`cx`, krate, version, field); the provider's `Cargo.toml` already allows `too_many_lines` and related. If a lint is raised from a category not already allowed in the provider's `[lints.clippy]` block, prefer fixing the code rather than broadening the allow-list.

## Risks / gotchas

### crates.io specifics

- **Sparse index path encoding.** `CrateName::sparse_index_path()` in `types.rs` encodes the length-prefix bucket structure crates.io's sparse index requires (1/a, 2/ab, 3/a/abc, 4+/ab/cd/abcd). Preserved unchanged. Any change to `CrateName::from_str`'s normalization (currently lowercases) must preserve the contract that the resulting path is lookup-parity with `lower(raw_name)`.
- **Sparse index vs v1 API.** Version data is pulled from `index.crates.io` (sparse index, raw JSONL with `vers`/`cksum`/`deps`/`features`/`features2`/`yanked`/`rust_version`/`pubtime`), while summary and owner data come from `https://crates.io/api/v1`. The two have different schemas; `SparseVersionRecord` and `VersionRecord` bridge them. Keep both call sites on the client or the JSONL parser becomes a cross-cutting concern.
- **Yanked versions.** `SparseVersionEntry.yanked` is authoritative. Expose both per-version (`versions/{version}/yanked`) and per-version-record-derived (`VersionRecord.yanked`) so that listing the versions dir yields both yanked and non-yanked versions; consumer tooling can filter.
- **`features2` compatibility.** The sparse index uses `features2` as an overflow map for feature flags that contain characters outlawed in `features`. The old client merges `features2` into `features` before returning. Preserved as-is; do not split them back apart in the new handlers.
- **User-Agent.** The old client hardcodes `USER_AGENT = "omnifs-provider-crates-io/0.1.0"` and sends it on every request. crates.io returns 403 without a User-Agent. Preserved by keeping `client.rs` verbatim. Do not drop the header in any new code path.
- **Search pagination.** The old `SearchResultsView` carries `next_page` but the dir handler always projects a single `results.json` with page=1 (`page=1&per_page={search_page_size}`) and marks the dir exhaustive. Carried over. A later extension could add `?page=N` handling via `DirCx::intent()` and a `Cursor::Page(n)` pagination scheme.
- **Rate limits.** crates.io returns 429 under load. `ProviderError::from_http_status(429)` returns a `RateLimited` error which is retryable by the host. No provider-side backoff is needed (nor allowed).

### SDK-shape gotchas to watch for

- **`Projection::file` vs `file_with_content`.** `file(name)` leaves the file as a placeholder (4096-byte stat, no bytes); the host then has to call `read_file` to fetch content. Use `file_with_content(name, bytes)` whenever bytes are already in hand; this lets the host short-circuit subsequent reads. The version/owner handlers above already use `file_with_content` everywhere for this reason.
- **Projected file size cap.** `MAX_PROJECTED_BYTES = 64 KiB`. The crate summary and version records are well under this; `keywords.json`/`categories.json` are tiny JSON arrays; `features.json`/`dependencies.json` can grow for large crates (e.g. `async-std`, `tokio`), but should stay under 64 KiB. If a real crate trips this cap, move the offending field to a direct `#[file]` handler (bypasses the projection limit; content is streamed directly in the terminal rather than listed ahead).
- **Project sibling files on every read.** The project CLAUDE.md states "Project sibling files on every read where you already know them." The `version_field` and `owner_field` file handlers above fetch the full record and only return one field's bytes. Consider wrapping the return in `FileContent::with_sibling_files(...)` populated with the other fields' bytes so the host caches the whole record in one go. Left as an optional enhancement; do not add it in this migration unless verification fails because of repeated fetches.
- **`cx.state` closure return type.** Closures passed to `cx.state` cannot borrow across await points. Build any `Result<Vec<_>>` inside the closure and `?` outside. The search handler above does this correctly by letting `CratesIoClient::new(cx)` capture `&Cx<State>` (the client's methods only read state at call time, never across suspensions).
- **`FromStr` for captures is `Result<Self, ()>`.** The new capture types mirror this. The SDK treats `Err(())` as a routing miss (returns `not-found` without calling the handler), which is exactly what's needed for `VersionField` and `OwnerField`.

### Merge/rebase risk

- The hard-reset step in "Bring worktree up to main" discards the worktree's host/SDK/CLI work. That work is obsoleted by main's redesign; there is no salvageable content. If a specific piece of the worktree's work needs to be salvaged, surface it before running the reset; do not attempt a merge (the conflict surface area is too large for correct resolution without picking sides wholesale, which is what the hard-reset already does).
- Back up the provider files under `/tmp/crates-io-migration-save/` BEFORE the reset. The provider directory is untracked in the current working tree per `git status`, so a reset won't nuke it, but the backup is cheap insurance against a stale mental model.
- After the reset, the branch `wip/provider-crates-io-impl` will effectively become "main + crates-io provider". That is the desired state. Push with `-u` when ready; `--force-with-lease` is sufficient.

---

## Verification

- `cargo fmt --check`
- `cargo clippy -p omnifs-provider-crates-io --target wasm32-wasip2 -- -D warnings`
- `cargo test -p omnifs-provider-crates-io --target wasm32-wasip2 --no-run`
- `just check-providers`

All must pass. If `just` is not on PATH, note that in the PR body and run
the equivalent `cargo` commands from the root of this branch's worktree.

## Commit

Conventional:

```
feat(crates-io): migrate provider to path-first handler SDK
```

Body: one paragraph naming the major structural changes and the base SDK
PRs (#28 `feat/sdk-http-post-support`, #29 `feat/sdk-path-rest-captures`,
#D `feat/sdk-error-constructors`).

## PR

- Title: `feat(crates-io): migrate provider to path-first handler SDK`
- Body: summary + link to this branch's `PLAN.md` + verification results
  (which cargo commands ran, which passed, which were skipped and why).
