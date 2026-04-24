# feat/migrate-notion

The `notion` worktree (`/Users/raul/W/gvfs/.worktrees/providers/notion`, tip `e1d0b85`, fork point `7742e99`) was written against the OLD mount- table SDK (`omnifs_sdk::mounts!` + `Dir` / `File` traits + `SingleEffect` / `EffectFuture` HTTP, plus `CacheInvalidatePrefix` effects).

## Blocked by

This plan cannot start execution until all three of these have merged into `main`:

- PR #28 `feat/sdk-http-post-support` — https://github.com/raulk/omnifs/pull/28
- PR #29 `feat/sdk-path-rest-captures` — https://github.com/raulk/omnifs/pull/29
- PR TBD `feat/sdk-error-constructors` — error constructor convenience methods

## Execution model

This branch was created off `main` at `6343486`. To execute:

1. `git -C /Users/raul/W/gvfs worktree add /Users/raul/W/gvfs/.worktrees/migrate-notion feat/migrate-notion`
2. Work in that worktree only.
3. Bring in the provider source from the old worktree at
   `/Users/raul/W/gvfs/.worktrees/providers/notion/providers/notion/`
   per the "Port provider source" step below.
4. Execute this PLAN.md end-to-end. Corrections in the "Migration
   corrections" section are authoritative over anything in the reference
   body that contradicts them.
5. Run the Verification commands listed near the bottom.
6. Commit on the `feat/migrate-notion` branch, push, open PR.


## Port provider source

This branch is off `main` at `6343486`, so there is NO merge from
`wip/provider-notion-impl` and NO `git merge main`. The wip branch carries OLD-SDK infrastructure
that must not land here. Only provider-local files come over, file by file,
using `git checkout <old-branch> -- <path>` (this pulls the file contents into
the working tree and index without touching anything else).

### Files to copy verbatim (no touch-ups beyond rust import paths / `ProviderResult` → `Result`)

- `providers/notion/src/types.rs`

Bring each over with:

```bash
git checkout wip/provider-notion-impl -- providers/notion/src/types.rs
```

### Files to copy then touch up

- `providers/notion/src/api.rs` — copy over, then apply the corrections in
  the relevant reference-body sections (auth removal, POST rewrites, error
  constructor substitutions).

Bring them in with:

```bash
git checkout wip/provider-notion-impl -- providers/notion/src/api.rs
```

Then edit in place.

### Files to create fresh (do NOT copy from the wip branch)

- `providers/notion/src/lib.rs`
- `providers/notion/src/provider.rs`
- `providers/notion/src/root.rs`
- `providers/notion/src/handlers/ (databases, pages, blocks, users)`

### Files to DISCARD (do NOT bring to this branch)

- `providers/notion/src/http_ext.rs`
- `providers/notion/src/fs.rs`
- `providers/notion/src/old provider.rs`
- `providers/notion/src/old lib.rs`

These are old-SDK artifacts (entity projections, tree walkers, routes tables,
manual http_ext wrappers for auth). The new SDK shape replaces them with
path-first handlers.

### Bring over the provider Cargo.toml

```bash
git checkout wip/provider-notion-impl -- providers/notion/Cargo.toml
```

Then update its SDK dependency declarations to match `providers/github/Cargo.toml`
on the current `main`. In particular, `omnifs-sdk` must point at the workspace
version and not an old path/git revision.

### Re-register the provider in the workspace

The workspace-level `Cargo.toml` on `main` dropped every non-dns/github/test
provider. Re-add `providers/notion` to its `members` array. Example diff:

```toml
[workspace]
members = [
    "crates/cli",
    "crates/host",
    "providers/dns",
    "providers/github",
+   "providers/notion",
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

The host injects the `Authorization: Bearer <token>` header via
`AuthManager::headers_for_url`. The provider declares intent in
`capabilities()` and does nothing else for auth:

```rust
Capabilities {
    auth_types: vec!["bearer-token".to_string()],
    domains: vec!["api.notion.com".to_string()],
    ..Default::default()
}
```

Remove every one of these from the original plan's `Config`, `State`,
`http_ext.rs`, and handler code:

- `token`, `api_key`, `integration_token`, `oauth_access_token` fields on
  `Config` or `State` (unless the token is used for something other than
  Authorization header injection; if so, call it out explicitly in PR
  description).
- Any manual `.header("Authorization", format!("Bearer {token}"))`,
  `.header("Authorization", api_key)`, or equivalent manual injection.
- Any "thread token through State/Config" bullet or code snippet in the
  original plan body; it is superseded.

Keep any non-auth headers the API requires (for example notion's
`Notion-Version`, github's `X-GitHub-Api-Version`, gmail's `Accept:
application/json`). Those are still the provider's responsibility.

The canonical model is `providers/github/src/http_ext.rs` on `main`: no
token-passing, no `Authorization` injection, just the provider's own
versioning and content-type headers.

Domains covered:

  - `api.notion.com`

Mount config shape the user supplies:

```json
{
  "plugin": "notion.wasm",
  "mount": "/notion",
  "auth": [{"type": "bearer-token", "token_env": "NOTION_API_KEY", "domain": "api.notion.com"}]
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

---

## Reference body (original MIGRATION_PLAN.md; subordinate to the corrections above)

> The content that follows was written for the old-SDK worktree at
> `/Users/raul/W/gvfs/.worktrees/providers/notion/MIGRATION_PLAN.md`.
> Read it for provider-specific shape, path tables, gotchas, and per-file
> migration notes. Wherever a passage conflicts with the corrections above
> (auth handling, POST shape, error constructors, rest captures, destructive
> action for crates-io), the corrections win.

# Notion provider migration plan

## Summary

The `notion` worktree (`/Users/raul/W/gvfs/.worktrees/providers/notion`,
tip `e1d0b85`, fork point `7742e99`) was written against the OLD mount-
table SDK (`omnifs_sdk::mounts!` + `Dir` / `File` traits + `SingleEffect`
/ `EffectFuture` HTTP, plus `CacheInvalidatePrefix` effects). Main
(`6343486`) has replaced that surface with free-function handlers
(`#[handlers] impl X { #[dir("...") ] ... }`), an owned `Projection`
builder, request/response `Callout`s, and `EventOutcome` for
invalidation.

The migration merges main into the worktree (taking main's versions of
`crates/` and `wit/` verbatim), adds `providers/notion` back to the
workspace `members` list, rewrites the provider code against the new
SDK, and keeps the existing Notion API client (`api.rs`) and path types
(`types.rs`) intact. The browse shape is preserved:

```
/
  _pages/{page_id}/{title,properties.json,content.md}
  _shared_pages/{shared_page}/{title,properties.json,content.md}
```

No source code is modified by this document; a sonnet executor applies
the per-file replacements below.

## Current path table (verbatim from old `mounts!`)

| Path template                                       | Kind  | OLD handler type          |
|-----------------------------------------------------|-------|---------------------------|
| `/`                                                 | dir   | `Root`                    |
| `/_pages`                                           | dir   | `Pages`                   |
| `/_pages/{page_id}`                                 | dir   | `Page`                    |
| `/_pages/{page_id}/title`                           | file  | `PageTitle`               |
| `/_pages/{page_id}/properties.json`                 | file  | `PageProperties`          |
| `/_pages/{page_id}/content.md`                      | file  | `PageContent`             |
| `/_shared_pages`                                    | dir   | `SharedPages`             |
| `/_shared_pages/{shared_page}`                      | dir   | `SharedPage`              |
| `/_shared_pages/{shared_page}/title`                | file  | `SharedPageTitle`         |
| `/_shared_pages/{shared_page}/properties.json`      | file  | `SharedPageProperties`    |
| `/_shared_pages/{shared_page}/content.md`           | file  | `SharedPageContent`       |

Captures declared in old `mounts!`:

```
capture page_id: crate::types::PageId;
capture shared_page: crate::types::SharedPageName;
```

## Target path table (NEW SDK)

Same paths, same captures; the handlers move from trait `impl`s to free
functions decorated with `#[dir]` / `#[file]` inside `#[handlers] impl
NotionHandlers`. Captures are typed through `FromStr` (both `PageId` and
`SharedPageName` already implement `FromStr<Err = String>`, which
satisfies the SDK requirement).

| Path template                                       | Kind  | NEW handler fn                |
|-----------------------------------------------------|-------|-------------------------------|
| `/`                                                 | dir   | `root`                        |
| `/_pages`                                           | dir   | `pages_dir`                   |
| `/_pages/{page_id}`                                 | dir   | `page_dir`                    |
| `/_pages/{page_id}/title`                           | file  | `page_title_file`             |
| `/_pages/{page_id}/properties.json`                 | file  | `page_properties_file`        |
| `/_pages/{page_id}/content.md`                      | file  | `page_content_file`           |
| `/_shared_pages`                                    | dir   | `shared_pages_dir`            |
| `/_shared_pages/{shared_page}`                      | dir   | `shared_page_dir`             |
| `/_shared_pages/{shared_page}/title`                | file  | `shared_page_title_file`      |
| `/_shared_pages/{shared_page}/properties.json`      | file  | `shared_page_properties_file` |
| `/_shared_pages/{shared_page}/content.md`           | file  | `shared_page_content_file`    |

## SDK cheatsheet (NEW)

Inline, verbatim. The sonnet executor should treat this as the spec
for every code change below.

### Provider skeleton

```rust
use omnifs_sdk::prelude::*;

#[provider(mounts(crate::handlers::NotionHandlers))]
impl NotionProvider {
    fn init(config: Config) -> Result<(State, ProviderInfo)> { /* ... */ }
    fn capabilities() -> RequestedCapabilities { /* ... */ }
    async fn on_event(cx: Cx<State>, event: ProviderEvent) -> Result<EventOutcome> { /* ... */ }
}
```

### Handlers

```rust
use omnifs_sdk::prelude::*;

pub struct NotionHandlers;

#[handlers]
impl NotionHandlers {
    #[dir("/")]
    fn root(_cx: &DirCx<'_, State>) -> Result<Projection> { /* ... */ }

    #[dir("/_pages/{page_id}")]
    async fn page_dir(cx: &DirCx<'_, State>, page_id: PageId) -> Result<Projection> { /* ... */ }

    #[file("/_pages/{page_id}/content.md")]
    async fn page_content_file(cx: &Cx<State>, page_id: PageId) -> Result<FileContent> { /* ... */ }
}
```

Rules:

- Handler fns are free functions (no `self`). First parameter is
  `&DirCx<'_, State>` for `#[dir]` or `&Cx<State>` for `#[file]`.
  `DirCx` derefs to `Cx`, so `cx.http()`, `cx.state()`, etc. work on
  both.
- Typed captures use `FromStr` when the type is not `String`.
  `PageId` and `SharedPageName` already implement `FromStr<Err =
  String>` — no change needed.
- Handler fns may be sync (`fn`) or `async fn`.

### Projection builder (owned)

```rust
let mut p = Projection::new();
p.dir("entries");
p.file("lazy.txt");                     // placeholder stat, lazy read
p.file_with_stat("known.bin", FileStat { size: NonZeroU64::new(1024).unwrap() });
p.file_with_content("schema.json", bytes); // eager, must be <= 64 KiB
p.page(PageStatus::Exhaustive);
// or: p.page(PageStatus::More(Cursor::Opaque("next".into())));
p.preload("some/other/path", bytes);
p.preload_many([("a", b"x".to_vec()), ("b", b"y".to_vec())]);
```

`file_with_content` records an error inside the projection if `bytes`
exceed `MAX_PROJECTED_BYTES` (64 KiB). Files that might exceed that
must be served by a `#[file]` handler instead.

### File content terminal

```rust
Ok(FileContent::bytes(bytes))
Ok(FileContent::bytes(bytes).with_sibling_files([
    ProjectedFile::new("title", title_bytes),
    ProjectedFile::new("properties.json", props_bytes),
]))
```

### HTTP

```rust
// GET only, via the typed builder:
let bytes = cx.http()
    .get(url)
    .header("Notion-Version", version)
    .header("Accept", "application/json")
    .send_body().await?;            // -> Vec<u8>

let resp = cx.http()
    .get(url)
    .header("...", "...")
    .send().await?;                 // -> HttpResponse { status, headers, body }
```

**Important: the new `http::Builder` only exposes `.get()`.** There is
no `.post()` and no `.body()` / `.json()` setter on `Request`. POST
bodies must be assembled as a raw `Callout::Fetch(HttpRequest { .. })`
exactly the way `providers/dns/src/doh.rs` does. See the `post_json`
helper in the rewritten `http_ext.rs` below.

Parallelism is explicit: `omnifs_sdk::prelude::join_all(iter_of_futs)`
runs N callout futures through a single yield/resume cycle.

### Context

```rust
let version = cx.state(|s| s.notion_version.clone());
cx.state_mut(|s| { s.etag = Some(new_etag); });
```

### Errors

```rust
use omnifs_sdk::prelude::*;

ProviderError::not_found("page not found")
ProviderError::invalid_input("bad uuid")
ProviderError::internal("unexpected state")
ProviderError::not_a_directory("...")
ProviderError::not_a_file("...")
ProviderError::unimplemented("...")
```

`Result<T>` is re-exported from the prelude; it is
`core::result::Result<T, ProviderError>`.

### Events / invalidation

```rust
async fn on_event(_cx: Cx<State>, _event: ProviderEvent) -> Result<EventOutcome> {
    let mut outcome = EventOutcome::new();
    outcome.invalidate_prefix("_shared_pages");
    // outcome.invalidate_path("_pages/<id>/content.md");
    Ok(outcome)
}
```

There are no more `CacheInvalidate*` callouts; the host applies
invalidations carried on the `EventOutcome` return value at the
response boundary. Scope / identity invalidation is gone.

## Bring the worktree up to main

The new SDK, WIT, and provider reference implementations live on main.
Merge main into the worktree and accept main's versions of `crates/`
and `wit/`.

```bash
cd /Users/raul/W/gvfs/.worktrees/providers/notion

# Make sure we have main up to date.
git fetch origin main

# Sanity: worktree tip and fork point.
git rev-parse HEAD                           # e1d0b85...
git merge-base HEAD main                     # 7742e99...

# Merge main. Expect conflicts in:
#   crates/omnifs-sdk/**          (whole SDK redesigned)
#   crates/omnifs-sdk-macros/**   (new proc macros)
#   crates/host/**                (runtime redesigned)
#   crates/cli/**                 (follows host)
#   wit/**                        (callouts + event-outcome + handlers)
#   providers/github/**           (rewritten on main)
#   providers/dns/**              (rewritten on main)
#   providers/test/**             (rewritten on main)
#   Cargo.toml                    (workspace members)
#   Cargo.lock                    (regenerated)
git merge main
# If merge complains about untracked files under crates/ or wit/, commit
# or stash them first; do not discard without inspection.

# Take main's version wholesale for SDK, WIT, host, CLI, and other
# providers. Our only work lives under providers/notion/**.
git checkout --theirs -- crates wit providers/github providers/dns providers/test
git add crates wit providers/github providers/dns providers/test

# The worktree's providers/notion source stays "ours" for now; we
# rewrite it against the new SDK in the next sections.
git checkout --ours -- providers/notion
git add providers/notion

# Root Cargo.toml will conflict because main doesn't list notion. Take
# main's file and then we'll edit members back in (see "Cargo.toml
# changes" below).
git checkout --theirs -- Cargo.toml
# Cargo.lock: take main's and regenerate after the source rewrite.
git checkout --theirs -- Cargo.lock
git add Cargo.toml Cargo.lock
```

Do not commit yet; the merge commit wraps up after the provider rewrite
and Cargo.toml edit land.

Confirmation: after `git checkout --theirs -- crates wit`, the
worktree's `crates/omnifs-sdk/src/prelude.rs` must match main's
(exports `Projection`, `DirCx`, `FileContent`, `EventOutcome`,
`ProviderEvent`, `provider`, `handlers`, `dir`, `file`, `subtree`,
`join_all`, etc.). If it still shows `mount::{Dir, File, FileBytes}` or
`SingleEffect`, the checkout did not land — redo it before proceeding.

## Per-file migration

The worktree provider lives at
`/Users/raul/W/gvfs/.worktrees/providers/notion/providers/notion/`.

### `providers/notion/src/lib.rs` — REWRITE

Delete the old `mounts!` invocation and `ProviderResult` type alias.
Reorganize modules: rename `fs.rs` → `handlers.rs` (it becomes the
single `#[handlers] impl NotionHandlers` file).

Full replacement:

```rust
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

pub(crate) use omnifs_sdk::prelude::Result;

mod api;
mod handlers;
mod http_ext;
mod provider;
pub(crate) mod types;

#[derive(Clone)]
pub(crate) struct State {
    pub notion_version: String,
}

#[omnifs_sdk::config]
pub struct Config {
    #[serde(default = "default_notion_version")]
    pub notion_version: String,
}

fn default_notion_version() -> String {
    "2022-06-28".to_string()
}
```

Notes:

- The old default `"2026-03-11"` is replaced with `"2022-06-28"` (the
  version the user's task spec calls out, and a real Notion API
  version). The default is a one-line change if a different pin is
  preferred.
- The `#[derive(Clone)]` line that preceded `#[omnifs_sdk::config]`
  in the old code is dropped; the config macro already expands to
  the appropriate derives (see `providers/dns/src/lib.rs` on main,
  which applies `#[omnifs_sdk::config]` without a `#[derive(Clone)]`
  wrapper).
- No `pub(crate) use fs::{ ... }` re-exports: handler types are gone.
- No `use omnifs_sdk::prelude::ProviderError;` at the top level — the
  prelude is imported from within `handlers.rs` and `provider.rs`.

### `providers/notion/src/provider.rs` — REWRITE

Full replacement:

```rust
use omnifs_sdk::Cx;
use omnifs_sdk::prelude::*;

use crate::{Config, Result, State};

#[provider(mounts(crate::handlers::NotionHandlers))]
impl NotionProvider {
    fn init(config: Config) -> Result<(State, ProviderInfo)> {
        Ok((
            State {
                notion_version: config.notion_version,
            },
            ProviderInfo {
                name: "notion-provider".to_string(),
                version: "0.1.0".to_string(),
                description: "Read-only Notion API provider".to_string(),
            },
        ))
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["api.notion.com".to_string()],
            auth_types: vec!["bearer-token".to_string()],
            max_memory_mb: 32,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 0,
        }
    }

    async fn on_event(_cx: Cx<State>, _event: ProviderEvent) -> Result<EventOutcome> {
        let mut outcome = EventOutcome::new();
        // The shared-page listing is a search over every page visible to the
        // integration token; drop it on any event so the next list hits the
        // API. Page-level caches are left alone; they key off PageId.
        outcome.invalidate_prefix("_shared_pages");
        Ok(outcome)
    }
}
```

Reference: `providers/github/src/provider.rs` (uses
`#[provider(mounts(...))]`, returns `Result<(State, ProviderInfo)>`,
defines `on_event`) and `providers/dns/src/provider.rs` (simpler
skeleton, no events).

### `providers/notion/src/http_ext.rs` — REWRITE

The old file used `SingleEffect::Fetch` and `EffectFuture` directly.
The new SDK exposes only `cx.http().get(...)`; POST must still use the
raw WIT types, which are re-exported at
`omnifs_sdk::omnifs::provider::types::{Callout, Header, HttpRequest}`.
See `providers/dns/src/doh.rs` on main, which constructs a GET via the
same path.

Full replacement:

```rust
use omnifs_sdk::Cx;
use omnifs_sdk::http::{CalloutFuture, Request};
use omnifs_sdk::omnifs::provider::types::{Callout, CalloutResult, Header, HttpRequest};
use omnifs_sdk::prelude::*;

use crate::State;

/// Convenience headers every Notion call needs.
pub(crate) trait NotionHttpExt {
    /// GET with `Notion-Version` and `Accept: application/json`.
    fn notion_get(&self, url: impl Into<String>) -> Request<'_, State>;

    /// POST a JSON body with the same Notion headers. Returns the raw
    /// response body on success.
    fn notion_post_json(
        &self,
        url: impl Into<String>,
        body: Vec<u8>,
    ) -> CalloutFuture<'_, State, Vec<u8>>;
}

impl NotionHttpExt for Cx<State> {
    fn notion_get(&self, url: impl Into<String>) -> Request<'_, State> {
        let notion_version = self.state(|s| s.notion_version.clone());
        self.http()
            .get(url)
            .header("Notion-Version", notion_version)
            .header("Accept", "application/json")
    }

    fn notion_post_json(
        &self,
        url: impl Into<String>,
        body: Vec<u8>,
    ) -> CalloutFuture<'_, State, Vec<u8>> {
        let notion_version = self.state(|s| s.notion_version.clone());
        CalloutFuture::new(
            self,
            Callout::Fetch(HttpRequest {
                method: "POST".to_string(),
                url: url.into(),
                headers: vec![
                    Header {
                        name: "Notion-Version".to_string(),
                        value: notion_version,
                    },
                    Header {
                        name: "Accept".to_string(),
                        value: "application/json".to_string(),
                    },
                    Header {
                        name: "Content-Type".to_string(),
                        value: "application/json".to_string(),
                    },
                ],
                body: Some(body),
            }),
            |result| match result {
                CalloutResult::HttpResponse(resp) if resp.status < 400 => Ok(resp.body),
                CalloutResult::HttpResponse(resp) => {
                    Err(ProviderError::from_http_status(resp.status))
                },
                CalloutResult::CalloutError(e) => Err(ProviderError::from_callout_error(&e)),
                _ => Err(ProviderError::internal("unexpected callout result type")),
            },
        )
    }
}
```

Rename map from the old symbols:

- `SingleEffect` → `Callout`
- `SingleEffectResult` → `CalloutResult`
- `HttpResponse` match stays the same (path moved to
  `omnifs_sdk::omnifs::provider::types::HttpResponse`; prelude does not
  re-export it)
- `EffectFuture::new(..)` → `CalloutFuture::new(..)`
- `ProviderError::from_effect_error` → `ProviderError::from_callout_error`
- `err(..)` wrapper: dropped — the closures in `CalloutFuture` return
  `Result<T>` directly (no double-wrap). Confirm against
  `crates/omnifs-sdk/src/http.rs` on main.

**Bearer token injection.** The old code did not add the
`Authorization: Bearer <token>` header; it relied on the host runtime
to inject auth. The new SDK has no host-side auth injection visible in
the codebase; the `bearer-token` entry in `capabilities().auth_types`
is declarative only. The integration token must be carried in `State`
and added as a header by the provider. Extend `Config` and `State`
accordingly — see the "Config widening" sub-step under `lib.rs` if you
want this today; otherwise a TODO is acceptable if the host is known
to still inject auth, but do NOT leave unauthenticated calls in prod.

Pragmatic recommendation: add an `integration_token: String` field to
`Config` and `State`, and add `.header("Authorization", format!("Bearer
{token}"))` inside `notion_get` / `notion_post_json`. This matches what
the task spec's `Notion provider` sample shows.

Add these three lines to `lib.rs`'s `Config` and `State`:

```rust
// lib.rs
#[omnifs_sdk::config]
pub struct Config {
    pub integration_token: String,                        // NEW
    #[serde(default = "default_notion_version")]
    pub notion_version: String,
}

#[derive(Clone)]
pub(crate) struct State {
    pub integration_token: String,                        // NEW
    pub notion_version: String,
}
```

And update `provider.rs::init`:

```rust
Ok((
    State {
        integration_token: config.integration_token,
        notion_version: config.notion_version,
    },
    ProviderInfo { /* unchanged */ },
))
```

Then in `http_ext.rs`, the `notion_get` / `notion_post_json` bodies
pull the token:

```rust
let (token, version) = self.state(|s| (s.integration_token.clone(), s.notion_version.clone()));
self.http()
    .get(url)
    .header("Authorization", format!("Bearer {token}"))
    .header("Notion-Version", version)
    .header("Accept", "application/json")
```

### `providers/notion/src/api.rs` — KEEP (minor edits)

The Notion API client stays. Changes:

1. Replace the crate-local `ProviderResult<T>` alias import with the
   SDK's `Result`:
   ```rust
   use crate::{Result, State};
   ```
   and change every `ProviderResult<T>` return to `Result<T>`.

2. The `Cursor` import stays — the SDK re-exports `Cursor` from
   `omnifs_sdk::prelude::*`, so `use omnifs_sdk::prelude::Cursor;` also
   works. Prefer:
   ```rust
   use omnifs_sdk::prelude::{Cursor, ProviderError};
   ```

3. `cx.notion_get(url).send_body().await` — unchanged. Works verbatim
   with the new `Request::send_body` signature.

4. `cx.notion_post_json(url, body).await` — unchanged (same surface).

5. `cursor_string` function: no semantic change. Keep as-is.

6. Tests (`#[cfg(test)] mod tests`): no changes needed. `parse_model`
   and `page_title` and `properties_json` all stay.

The entire public surface of `api.rs` (`NotionClient`, `PageWire`,
`PageMarkdownWire`, `PageSearchResponse`, `page_title`,
`properties_json`) is preserved. No other provider code depends on its
internals beyond these names.

Diff-level edits to apply:

```
- use omnifs_sdk::browse::Cursor;
- use omnifs_sdk::prelude::ProviderError;
+ use omnifs_sdk::prelude::{Cursor, ProviderError};
- use crate::{ProviderResult, State};
+ use crate::{Result, State};
```

And replace every `ProviderResult<` with `Result<` in the file
(impl block signatures and helpers).

### `providers/notion/src/types.rs` — KEEP UNCHANGED

`PageId` and `SharedPageName` already implement
`Display + FromStr<Err = String> + Clone + Eq + Hash + Ord`. The SDK's
typed captures accept any `FromStr` whose error is `Display + Send +
Sync + 'static` — `String` qualifies. Nothing to change.

Tests in this file stay put.

### `providers/notion/src/fs.rs` — DELETE AND REPLACE WITH `handlers.rs`

`fs.rs` is the old mount-table trait wiring (`impl Dir for ...`, `impl
File for ...`, `Projection<'_, Self::Path>`, `load` / `project` / `read`
trait methods, `FileBytes`). Delete it.

Create `providers/notion/src/handlers.rs` with full contents:

```rust
use omnifs_sdk::prelude::*;

use crate::api::{NotionClient, page_title, properties_json};
use crate::types::{PageId, SharedPageName};
use crate::{Result, State};

pub struct NotionHandlers;

#[handlers]
impl NotionHandlers {
    // -------- static shape --------

    #[dir("/")]
    fn root(_cx: &DirCx<'_, State>) -> Result<Projection> {
        let mut p = Projection::new();
        p.dir("_pages");
        p.dir("_shared_pages");
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/_pages")]
    fn pages_dir(_cx: &DirCx<'_, State>) -> Result<Projection> {
        // /_pages is a namespace prefix; children are addressed by
        // explicit page id via /_pages/{page_id}. Listing is not
        // supported (same behavior as the old Pages dir which
        // returned an empty projection).
        let mut p = Projection::new();
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    // -------- per-page subtree (by explicit id) --------

    #[dir("/_pages/{page_id}")]
    async fn page_dir(cx: &DirCx<'_, State>, page_id: PageId) -> Result<Projection> {
        // Fetch once, project title+properties eagerly, leave content.md lazy.
        let page = NotionClient::new(cx).get_page(&page_id).await?;
        let title = page_title(&page).into_bytes();
        let props = properties_json(&page)?;

        let mut p = Projection::new();
        p.file_with_content("title", title);
        p.file_with_content("properties.json", props);
        p.file("content.md"); // large / dynamic; served by #[file] handler
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[file("/_pages/{page_id}/title")]
    async fn page_title_file(cx: &Cx<State>, page_id: PageId) -> Result<FileContent> {
        let page = NotionClient::new(cx).get_page(&page_id).await?;
        let title = page_title(&page).into_bytes();
        let props = properties_json(&page)?;
        Ok(FileContent::bytes(title.clone()).with_sibling_files([
            ProjectedFile::new("title", title),
            ProjectedFile::new("properties.json", props),
        ]))
    }

    #[file("/_pages/{page_id}/properties.json")]
    async fn page_properties_file(cx: &Cx<State>, page_id: PageId) -> Result<FileContent> {
        let page = NotionClient::new(cx).get_page(&page_id).await?;
        let title = page_title(&page).into_bytes();
        let props = properties_json(&page)?;
        Ok(FileContent::bytes(props.clone()).with_sibling_files([
            ProjectedFile::new("title", title),
            ProjectedFile::new("properties.json", props),
        ]))
    }

    #[file("/_pages/{page_id}/content.md")]
    async fn page_content_file(cx: &Cx<State>, page_id: PageId) -> Result<FileContent> {
        let markdown = NotionClient::new(cx)
            .get_page_markdown(&page_id)
            .await?
            .into_markdown()
            .into_bytes();
        Ok(FileContent::bytes(markdown))
    }

    // -------- /_shared_pages: search-backed listing --------

    #[dir("/_shared_pages")]
    async fn shared_pages_dir(cx: &DirCx<'_, State>) -> Result<Projection> {
        // Honor the host's paging cursor from DirIntent::List.
        let cursor = match cx.intent() {
            DirIntent::List { cursor } => cursor.clone(),
            _ => None,
        };
        let response = NotionClient::new(cx).search_pages(cursor).await?;

        let mut entries: Vec<SharedPageName> = response
            .results
            .iter()
            .map(|page| {
                let title = page_title(page);
                let id = page.page_id()?;
                Ok::<_, ProviderError>(SharedPageName::from_title(&title, id))
            })
            .collect::<Result<Vec<_>>>()?;
        entries.sort();

        let mut p = Projection::new();
        for entry in entries {
            p.dir(entry.to_string());
        }
        match response.next_cursor {
            Some(cursor) => p.page(PageStatus::More(Cursor::Opaque(cursor))),
            None => p.page(PageStatus::Exhaustive),
        }
        Ok(p)
    }

    #[dir("/_shared_pages/{shared_page}")]
    async fn shared_page_dir(
        cx: &DirCx<'_, State>,
        shared_page: SharedPageName,
    ) -> Result<Projection> {
        let page = NotionClient::new(cx).get_page(shared_page.page_id()).await?;
        let title = page_title(&page).into_bytes();
        let props = properties_json(&page)?;

        let mut p = Projection::new();
        p.file_with_content("title", title);
        p.file_with_content("properties.json", props);
        p.file("content.md");
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[file("/_shared_pages/{shared_page}/title")]
    async fn shared_page_title_file(
        cx: &Cx<State>,
        shared_page: SharedPageName,
    ) -> Result<FileContent> {
        let page = NotionClient::new(cx).get_page(shared_page.page_id()).await?;
        let title = page_title(&page).into_bytes();
        let props = properties_json(&page)?;
        Ok(FileContent::bytes(title.clone()).with_sibling_files([
            ProjectedFile::new("title", title),
            ProjectedFile::new("properties.json", props),
        ]))
    }

    #[file("/_shared_pages/{shared_page}/properties.json")]
    async fn shared_page_properties_file(
        cx: &Cx<State>,
        shared_page: SharedPageName,
    ) -> Result<FileContent> {
        let page = NotionClient::new(cx).get_page(shared_page.page_id()).await?;
        let title = page_title(&page).into_bytes();
        let props = properties_json(&page)?;
        Ok(FileContent::bytes(props.clone()).with_sibling_files([
            ProjectedFile::new("title", title),
            ProjectedFile::new("properties.json", props),
        ]))
    }

    #[file("/_shared_pages/{shared_page}/content.md")]
    async fn shared_page_content_file(
        cx: &Cx<State>,
        shared_page: SharedPageName,
    ) -> Result<FileContent> {
        let markdown = NotionClient::new(cx)
            .get_page_markdown(shared_page.page_id())
            .await?
            .into_markdown()
            .into_bytes();
        Ok(FileContent::bytes(markdown))
    }
}
```

Notes on the rewrite:

- `pages_dir` mirrors the old `Pages` dir which had an empty `project`.
  Listing `/_pages` was never meaningful — pages are addressed by
  explicit id. Kept for path coverage.
- `page_dir` and `shared_page_dir` eagerly project `title` and
  `properties.json` as inline `file_with_content` entries. They stay
  within the 64 KiB eager cap (typical Notion property blocks are well
  under this; if you ever hit it, swap `file_with_content` for
  `file_with_stat` and let the `#[file]` handlers serve on demand).
- The per-file handlers return `FileContent::with_sibling_files` so a
  read of `title` warms `properties.json` (and vice versa) at the cost
  of one already-fetched page.
- `content.md` is served lazily by its own `#[file]` handler because
  the Notion `GET /pages/{id}/markdown` response can be large.
- The shared-pages listing preserves the old behavior (sorted entries,
  `next_cursor` → `PageStatus::More(Cursor::Opaque(..))`). The cursor
  from the host arrives via `DirCx::intent()` as
  `DirIntent::List { cursor }`.
- The old `validate_page` helper (`load` used to issue `GET /pages/{id}`
  just to confirm existence) is gone: `page_dir` now makes the same
  call and uses the payload.

### `providers/notion/Cargo.toml` — NO CHANGE

The existing manifest matches the main-branch pattern
(`providers/github/Cargo.toml`, `providers/dns/Cargo.toml`). It already
has:

- `crate-type = ["cdylib", "lib"]`
- `omnifs-sdk = { path = "../../crates/omnifs-sdk" }`
- `serde`, `serde_json`, `uuid`
- `[package.metadata.component]` (vestigial but documented)
- clippy lint block matching the other providers

Optional additions (only if you end up needing them; github has both):
`hashbrown = "0.15"` for provider-internal maps, `strum = { version =
"0.27", features = ["derive"] }` for enum helpers. The rewrite above
does not require either.

## Cargo.toml changes (workspace root)

After `git checkout --theirs -- Cargo.toml` in the merge, the
workspace root file matches main. Re-add `providers/notion` to
`members`:

```toml
[workspace]
resolver = "2"
members = [
    "crates/*",
    "providers/github",
    "providers/dns",
    "providers/notion",
    "providers/test",
]
default-members = ["crates/cli", "crates/host"]
```

Diff:

```
     "providers/dns",
+    "providers/notion",
     "providers/test",
```

Leave `[workspace.dependencies]`, `[workspace.lints.*]` as main has
them — do not carry over anything else from the worktree's pre-merge
root. Notion-specific dependencies belong to `providers/notion/Cargo.toml`.

## Verification

Run in this order from the worktree root
(`/Users/raul/W/gvfs/.worktrees/providers/notion`):

```bash
# 1. Formatting.
cargo fmt --check

# 2. Regenerate the lockfile after workspace edits.
cargo generate-lockfile    # no-op if already coherent

# 3. Host crates must still build / lint cleanly on native.
cargo clippy -p omnifs-host -p omnifs-cli -- -D warnings
cargo test -p omnifs-host -p omnifs-cli

# 4. Provider clippy on wasm32-wasip2 (MUST pass with -D warnings).
cargo clippy -p omnifs-provider-notion --target wasm32-wasip2 -- -D warnings

# 5. Provider tests compile (cannot execute on wasm32-wasip2 host).
cargo test -p omnifs-provider-notion --target wasm32-wasip2 --no-run

# 6. The umbrella provider check used in CI.
just check-providers
```

If step 4 flags `clippy::too_many_lines` or `clippy::pedantic` issues
specific to `handlers.rs`, consult the `[lints.clippy]` block in
`providers/notion/Cargo.toml` — it already opts out of
`too_many_lines`, `module_name_repetitions`, and friends.

If step 5 reports `error[E0599]: no method named 'post' found for
struct 'Builder'`, the rewrite inadvertently called `cx.http().post()`.
That path does not exist in the new SDK; use the `Callout::Fetch(...)`
POST helper in `http_ext.rs`.

## Risks and gotchas

- **Notion-Version header pinning.** The new SDK still passes headers
  through untouched, so the existing pinning model (`notion_version`
  in `State`, injected by `notion_get` / `notion_post_json`) transfers
  directly. Leave the default at a known-good API version
  (`2022-06-28` matches the spec sample; the worktree's
  `"2026-03-11"` does not correspond to any published Notion API
  version and should not ship).

- **Bearer token.** As called out under `http_ext.rs`: there is no
  host-side auth injection on main. Add `integration_token` to
  `Config`/`State` and send
  `Authorization: Bearer <token>` on every call. Missing this will
  make every Notion call return 401.

- **Block tree recursion.** The Notion API returns blocks in a paged,
  shallow structure (`GET /v1/blocks/{id}/children`). The old provider
  used the higher-level `GET /v1/pages/{id}/markdown` endpoint which
  avoids explicit recursion. Keep using that endpoint via
  `NotionClient::get_page_markdown`; do not reimplement block walking
  inside the handler unless you are prepared to page children,
  recurse into nested blocks, and join their children callouts with
  `join_all`.

- **Large page content > 64 KiB.** `file_with_content` rejects any
  payload over `MAX_PROJECTED_BYTES` (64 KiB) and records an error on
  the projection. `content.md` can easily exceed that, which is why
  it is served by a dedicated `#[file]` handler and NEVER inlined
  into a projection. `title` (a few hundred bytes at worst) and
  `properties.json` (typically a few KiB) are safe to inline; guard
  against future `properties.json` blowups by switching to
  `file_with_stat` + lazy read if a page with very large properties
  is ever observed.

- **rich_text → markdown conversion.** The old code relies entirely on
  Notion's server-rendered `GET /pages/{id}/markdown` endpoint; no
  provider-side rich-text → markdown conversion happens. If that
  endpoint is ever deprecated, the migration does not cover
  implementing a local converter — it would be a separate project.

- **Pagination cursors.** `SharedPageName` listings pass Notion's
  opaque `next_cursor` back as `Cursor::Opaque(..)`. The old code
  rejected `Cursor::Page(..)` with `invalid_input`; the rewrite above
  silently drops any cursor it is given (matches how `github` and
  `dns` behave on main). If strict validation is desired, wrap the
  cursor extraction in `api.rs::cursor_string` (kept from the old
  code) and call it from `shared_pages_dir` before hitting the API.

- **`DirCx::intent()` for list cursors.** Previously `Dir::load`
  received `cursor: Option<Cursor>` as a parameter. On main that
  arrives via `cx.intent()` → `DirIntent::List { cursor }`. Lookup
  calls (`DirIntent::Lookup { child }`) and projected-file reads
  (`DirIntent::ReadProjectedFile { .. }`) do NOT carry a cursor; the
  handler must not branch on cursor absence to mean "first page" only
  — it may mean "this is a lookup, don't paginate at all". The
  rewrite defaults `cursor = None` except for `DirIntent::List`,
  which is the correct behavior.

- **No provider LRU / TTL.** The old `ProviderResult<T>` type was
  just an alias; the worktree has no self-owned cache to remove. If
  a future change introduces one, push invalidation through
  `EventOutcome::invalidate_prefix` / `invalidate_path` instead.

- **`CacheInvalidate*` effects no longer exist.** If any stray
  reference to `Effect::CacheInvalidatePrefix` or
  `Effect::CacheInvalidateIdentity` survives in `api.rs` after the
  merge, delete it — they are not in the new WIT or SDK.

- **`hashbrown` vs `std::collections::HashMap`.** Not currently used
  in the notion provider. If you introduce a map, use `hashbrown` per
  the project CLAUDE.md.

- **Projected file sizes must be non-zero.** `Projection::file_with_stat`
  takes a `FileStat { size: NonZeroU64 }`; `Projection::file` stamps a
  4096-byte placeholder. Both are safe against the "kernel sees 0 and
  never reads" gotcha.

- **Tests don't execute on wasm32-wasip2.** The tests in `api.rs` and
  `types.rs` that check pure functions (`page_title`, `PageId::parse`,
  `SharedPageName` round-trip) compile under
  `--target wasm32-wasip2 --no-run`. Do not add tests that rely on
  `tokio` or an async runtime to the notion crate — the provider ships
  as a WASM component with no embedded runtime.

---

## Verification

- `cargo fmt --check`
- `cargo clippy -p omnifs-provider-notion --target wasm32-wasip2 -- -D warnings`
- `cargo test -p omnifs-provider-notion --target wasm32-wasip2 --no-run`
- `just check-providers`

All must pass. If `just` is not on PATH, note that in the PR body and run
the equivalent `cargo` commands from the root of this branch's worktree.

## Commit

Conventional:

```
feat(notion): migrate provider to path-first handler SDK
```

Body: one paragraph naming the major structural changes and the base SDK
PRs (#28 `feat/sdk-http-post-support`, #29 `feat/sdk-path-rest-captures`,
#D `feat/sdk-error-constructors`).

## PR

- Title: `feat(notion): migrate provider to path-first handler SDK`
- Body: summary + link to this branch's `PLAN.md` + verification results
  (which cargo commands ran, which passed, which were skipped and why).
