# feat/migrate-google-drive

Migrate the google-drive provider from the OLD mount-table SDK to the NEW path-first / free-function handler SDK on main.

## Blocked by

This plan cannot start execution until both of these have merged into `main`:

- PR #28 `feat/sdk-http-post-support` — https://github.com/raulk/omnifs/pull/28
- PR #29 `feat/sdk-path-rest-captures` — https://github.com/raulk/omnifs/pull/29

Note: `ProviderError::rate_limited` / `::permission_denied` / `::version_mismatch`
constructors are already on `main` (landed with the #27 refactor). No separate PR
is needed.

## Execution model

This branch was created off `main` at `6343486`. To execute:

1. `git -C /Users/raul/W/gvfs worktree add /Users/raul/W/gvfs/.worktrees/migrate-google-drive feat/migrate-google-drive`
2. Work in that worktree only.
3. Bring in the provider source from the old worktree at
   `/Users/raul/W/gvfs/.worktrees/providers/google-drive/providers/google-drive/`
   per the "Port provider source" step below.
4. Execute this PLAN.md end-to-end. Corrections in the "Migration
   corrections" section are authoritative over anything in the reference
   body that contradicts them.
5. Run the Verification commands listed near the bottom.
6. Commit on the `feat/migrate-google-drive` branch, push, open PR.


## Port provider source

This branch is off `main` at `6343486`, so there is NO merge from
`wip/provider-google-drive-impl` and NO `git merge main`. The wip branch carries OLD-SDK infrastructure
that must not land here. Only provider-local files come over, file by file,
using `git checkout <old-branch> -- <path>` (this pulls the file contents into
the working tree and index without touching anything else).

### Files to copy verbatim (no touch-ups beyond rust import paths / `ProviderResult` → `Result`)

_None_; all source below is either touched-up or fresh.

### Files to copy then touch up

- `providers/google-drive/src/api.rs` — copy over, then apply the corrections in
  the relevant reference-body sections (auth removal, POST rewrites, error
  constructor substitutions).

Bring them in with:

```bash
git checkout wip/provider-google-drive-impl -- providers/google-drive/src/api.rs
```

Then edit in place.

### Files to create fresh (do NOT copy from the wip branch)

- `providers/google-drive/src/lib.rs`
- `providers/google-drive/src/provider.rs`
- `providers/google-drive/src/root.rs`
- `providers/google-drive/src/handlers/ (files, folders, shared-drives)`

### Files to DISCARD (do NOT bring to this branch)

- `providers/google-drive/src/http_ext.rs`
- `providers/google-drive/src/tree.rs`
- `providers/google-drive/src/old provider.rs`
- `providers/google-drive/src/old lib.rs`

These are old-SDK artifacts (entity projections, tree walkers, routes tables,
manual http_ext wrappers for auth). The new SDK shape replaces them with
path-first handlers.

### Bring over the provider Cargo.toml

```bash
git checkout wip/provider-google-drive-impl -- providers/google-drive/Cargo.toml
```

Then update its SDK dependency declarations to match `providers/github/Cargo.toml`
on the current `main`. In particular, `omnifs-sdk` must point at the workspace
version and not an old path/git revision.

### Re-register the provider in the workspace

The workspace-level `Cargo.toml` on `main` dropped every non-dns/github/test
provider. Re-add `providers/google-drive` to its `members` array. Example diff:

```toml
[workspace]
members = [
    "crates/cli",
    "crates/host",
    "providers/dns",
    "providers/github",
+   "providers/google-drive",
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
    domains: vec!["www.googleapis.com".to_string(), "drive.google.com".to_string()],
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

  - `www.googleapis.com`
  - `drive.google.com`

Mount config shape the user supplies:

```json
{
  "plugin": "google-drive.wasm",
  "mount": "/google-drive",
  "auth": [{"type": "bearer-token", "token_env": "GOOGLE_DRIVE_API_KEY", "domain": "www.googleapis.com"}]
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
> `/Users/raul/W/gvfs/.worktrees/providers/google-drive/MIGRATION_PLAN.md`.
> Read it for provider-specific shape, path tables, gotchas, and per-file
> migration notes. Wherever a passage conflicts with the corrections above
> (auth handling, POST shape, error constructors, rest captures, destructive
> action for crates-io), the corrections win.

# Google Drive provider: SDK migration plan

This document is an executable plan (target audience: Sonnet or a
comparable agent) for migrating the `google-drive` provider worktree
from the old mount-table SDK to the new path-first handler SDK that
landed on `main` (commit `6343486`, PR #27).

All code fragments are inlined. There are no external references to
follow. When a step says "write file X with the content below," paste
the block verbatim unless a concrete fact (e.g. a real file id in a
URL) clearly requires substitution.

Scope: do not touch the host runtime, the CLI, or the workspace root
layout beyond what this plan specifies.

---

## 1. Summary

- **Worktree**: `/Users/raul/W/gvfs/.worktrees/providers/google-drive`
- **Worktree tip**: `e1d0b85 fix(mounts): restore projected sibling file dispatch`
- **Fork point with main**: `7742e99 docs(readme): expand examples.`
- **Main tip**: `6343486 refactor!: redesign provider SDK and host runtime around path-first handlers and callouts`
- **Provider crate path**: `providers/google-drive/` (keep)
- **Provider source files (old)**: `api.rs`, `http_ext.rs`, `lib.rs`, `provider.rs`, `tree.rs`
- **Provider source files (new)**: `lib.rs`, `provider.rs`, `root.rs`, `items.rs`, `api.rs`, `http_ext.rs`, `types.rs` (rewrite/split; see section 6)

Migration shape:

1. Merge `main` into the worktree branch. This brings in the new
   `crates/omnifs-sdk`, `crates/omnifs-sdk-macros`, `crates/omnifs-mount-schema`,
   `crates/host`, `crates/cli`, and `wit/provider.wit`. Conflicts are
   expected in the SDK crates and the workspace root `Cargo.toml`; the
   provider's own `providers/google-drive/**` tree is not present on
   main so it merges as a pure addition.
2. Add `"providers/google-drive"` to the workspace root `Cargo.toml`
   `members` (the main-side `Cargo.toml` does not yet include it).
3. Rewrite the provider source against the new SDK (path-first
   `#[handlers]`, `Projection`, typed path captures, `EventOutcome`).
4. Verify with the provider clippy/test commands below.

The Drive API client (`DriveApi`, `FileMeta`, `WorkspaceKind`,
`ExportFormat`, `folder_children` disambiguation, export-format tables)
is preserved nearly verbatim: only the cache/LRU removal, the
`FileBytes`/`Entry`-from-SDK usage, and the `EntryStat` reference need
to be edited. The preload/sibling cache idioms that were only barely
used in the old provider are expanded in the rewrite so that lookups
materialize `meta.json` and folder listings carry projected blob bytes
where the budget allows.

---

## 2. Current path table (verbatim from old `mounts!`)

Copied from `providers/google-drive/src/lib.rs`:

```
omnifs_sdk::mounts! {
    capture file_id: String;

    "/" (dir) => Root;
    "/my-drive" (subtree) => MyDriveTree;
    "/_items" (dir) => ItemsRoot;
    "/_items/{file_id}" (subtree) => ItemTree;
}
```

- `/` is an empty root that advertises `my-drive` and `_items` as
  static children (not wired via `#[dir]` projection in the old code;
  the host relied on `routes!`-derived static children).
- `/my-drive` is a subtree rooted at the user's Drive root folder,
  resolved by walking folder children.
- `/_items` is an empty index directory.
- `/_items/{file_id}` is a subtree that exposes a single file id as
  `{meta.json, content, exports/*}`.

## 3. Target path table (new SDK)

The new SDK does not use `(subtree)` handoff for this provider.
Google Drive tree walking is an ordinary directory projection driven
by the folder listing API. Subtree handoff exists for git repositories
(handled by the host after a `Callout::GitOpenRepo`). Since Drive has
no git, every path is served by `#[dir]` / `#[file]` handlers.

| Path pattern | Handler kind | Module | Notes |
|---|---|---|---|
| `/` | `#[dir]` | `root::RootHandlers` | Lists `my-drive` and `_items`. Exhaustive. |
| `/my-drive` | `#[dir]` | `root::RootHandlers` | Lists children of Drive root folder (`"root"`). |
| `/my-drive/{segment1}` .. `/my-drive/{segment1}/{segment2}/...` | `#[dir]` per depth (see below) | `root::RootHandlers` | Resolves the segment chain and lists folder / workspace-doc content. |
| `/_items` | `#[dir]` | `items::ItemHandlers` | Empty index. Dynamic (no enumeration). |
| `/_items/{file_id}` | `#[dir]` | `items::ItemHandlers` | Projects `meta.json`, and either `content` (blob) or `exports/` (workspace doc). |
| `/_items/{file_id}/meta.json` | `#[file]` | `items::ItemHandlers` | Serialized `FileMeta`. |
| `/_items/{file_id}/content` | `#[file]` | `items::ItemHandlers` | Blob bytes via Drive `?alt=media`. |
| `/_items/{file_id}/exports` | `#[dir]` | `items::ItemHandlers` | Lists the export format names for workspace docs. |
| `/_items/{file_id}/exports/{format}` | `#[file]` | `items::ItemHandlers` | Exported workspace doc bytes. |

**`/my-drive` depth handling.** The new SDK resolves one template per
depth; it does not have an open-ended `{*tail}` catch-all. Drive
trees are arbitrarily deep. The plan is to expose only two levels of
structure under `/my-drive`:

- `#[dir("/my-drive")]` lists the top-level children of the Drive root
  folder.
- `#[dir("/my-drive/{name}")]` looks up one child of the Drive root
  by display name. For folders and workspace docs, the projection
  lists that child's contents (one level). For blobs, it returns a
  `Projection` that advertises a single `content` file (eager size,
  lazy bytes).

Deeper browsing is done via `/_items/{file_id}`, which is id-keyed and
independent of path depth. If the design requires deep path browsing
(several hierarchical levels under `/my-drive`), that is a follow-up
and should be surfaced to the user before it is implemented; it
requires either a depth ladder of handlers or an SDK addition for
open-ended captures. Do not invent one silently.

## 4. SDK cheatsheet (verbatim, inline)

### Provider

```rust
// lib.rs
pub(crate) use omnifs_sdk::prelude::Result;

mod provider;
mod root;
pub(crate) mod types;

#[derive(Clone)]
pub(crate) struct State { /* ... */ }

#[omnifs_sdk::config]
struct Config {
    oauth_access_token: String,
    #[serde(default = "default_page_size")] page_size: u32,
}
fn default_page_size() -> u32 { 100 }
```

```rust
// provider.rs
use omnifs_sdk::prelude::*;
use crate::{Config, State};

#[provider(mounts(crate::root::RootHandlers))]
impl GoogleDriveProvider {
    fn init(config: Config) -> Result<(State, ProviderInfo)> {
        Ok((State { /* ... */ }, ProviderInfo {
            name: "google-drive-provider".to_string(),
            version: "0.1.0".to_string(),
            description: "...".to_string(),
        }))
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["www.googleapis.com".to_string(), "drive.googleapis.com".to_string()],
            auth_types: vec![],
            max_memory_mb: 64,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 600,
        }
    }

    async fn on_event(cx: Cx<State>, event: ProviderEvent) -> Result<EventOutcome> {
        let mut outcome = EventOutcome::new();
        outcome.invalidate_prefix("/_folders");
        Ok(outcome)
    }
}
```

### Handlers

```rust
// root.rs
use omnifs_sdk::prelude::*;
use crate::{Result, State};

pub struct RootHandlers;

#[handlers]
impl RootHandlers {
    #[dir("/")]
    fn root(_cx: &DirCx<'_, State>) -> Result<Projection> {
        let mut p = Projection::new();
        p.dir("_folders");
        p.dir("_files");
        Ok(p)
    }

    #[dir("/_folders/{folder_id}")]
    async fn folder_dir(cx: &DirCx<'_, State>, folder_id: String) -> Result<Projection> {
        let token = cx.state(|s| s.oauth_access_token.clone());
        let bytes = cx.http()
            .get(format!("https://www.googleapis.com/drive/v3/files?q='{folder_id}'+in+parents"))
            .header("Authorization", format!("Bearer {token}"))
            .send_body().await?;
        let mut p = Projection::new();
        p.file_with_content("children.json", bytes);
        Ok(p)
    }

    #[file("/_files/{file_id}.bin")]
    async fn file_content(cx: &Cx<State>, file_id: String) -> Result<FileContent> {
        let token = cx.state(|s| s.oauth_access_token.clone());
        let bytes = cx.http()
            .get(format!("https://www.googleapis.com/drive/v3/files/{file_id}?alt=media"))
            .header("Authorization", format!("Bearer {token}"))
            .send_body().await?;
        Ok(FileContent::bytes(bytes))
    }
}
```

### Rules for handlers

- Path captures become typed args; non-`String` captures must implement `FromStr`.
- `DirCx<'_, S>` derefs to `Cx<S>`.
- Sync or `async fn` allowed.
- `Projection::new()` → `.dir(name)`, `.file(name)`, `.file_with_stat(name, FileStat { size: NonZeroU64::new(n).unwrap() })`, `.file_with_content(name, bytes)` (eager bytes ≤ 64 KiB), `.page(PageStatus::{Exhaustive, More(Cursor::Opaque(s))})`, `.preload(path, bytes)` / `.preload_many(iter)`.
- `Lookup::with_sibling_files(iter)` / `FileContent::with_sibling_files(iter)` for cache adjacency.
- Errors: `ProviderError::{not_found, invalid_input, internal, not_a_directory, not_a_file, unimplemented}`.

### Context

- `cx.state(|s| ...)` / `cx.state_mut(|s| ...)`.
- `cx.http()`: `.get/.post(url)`, `.header(k,v)`, `.json(&body)`, `.send_body().await -> Result<Vec<u8>>`, `.send().await -> Result<HttpResponse>`.
- `cx.git()` → `GitRepoInfo { tree_ref }` (not relevant here).
- `join_all(futs)` for parallel callouts.

### Caching model

- Host owns caching. No provider LRUs or TTLs.
- Non-zero file sizes. Placeholder size = 4096.
- Invalidation via `EventOutcome::invalidate_path`/`invalidate_prefix` returned from `on_event`. Scope/identity invalidation removed.

### OLD → NEW map (quick reference)

| OLD | NEW |
|-----|-----|
| `mounts! { "/p/{c}" (dir) => S; }` | `#[dir("/p/{c}")]` free fn in `#[handlers] impl S` |
| `impl Dir/File/Subtree for S` | Free-function handler returning `Result<Projection/FileContent/SubtreeRef>` |
| `Projection<'_, Path>` | `Projection` (owned) via prelude |
| `materialize()` | REMOVED (folds into lookup/list) |
| `routes!`, `#[lookup]`/`#[list]`/`#[read]` | REMOVED (compile error) |
| `Effect` / `SingleEffect` | `Callout` (request/response only) |
| `Effect::CacheInvalidate{Prefix,Identity,Scope}` | `EventOutcome` from `on_event`; scope/identity invalidation removed |
| `Effect::Git{ListTree, ReadBlob, HeadRef, ListCachedRepos}` | `Callout::GitOpenRepo` (host does tree walks) |
| Provider LRU/TTL | FORBIDDEN |
| `ProviderResult<T>` | `omnifs_sdk::prelude::Result` |
| `entities/`, `tree.rs` trait impls | `#[handlers] impl XxxHandlers` modules |

## 5. Bring the worktree up to main

Run from the worktree root (`/Users/raul/W/gvfs/.worktrees/providers/google-drive`).

```bash
# 1. Confirm clean state.
git status

# 2. Bring main refs into the worktree.
git fetch origin main

# 3. Merge main into the worktree branch. Expect conflicts in:
#    - crates/omnifs-sdk/**
#    - crates/omnifs-sdk-macros/**
#    - crates/omnifs-mount-schema/**
#    - crates/host/**
#    - crates/cli/**
#    - wit/provider.wit
#    - Cargo.toml  (workspace members)
#    - Cargo.lock
#    The provider's own providers/google-drive/** is not present on
#    main, so it merges as a pure addition.
git merge origin/main
```

Conflict resolution policy:

- In every conflict between the worktree's old SDK/wit and main's new
  SDK/wit, **take main verbatim**. The whole point of this migration
  is to adopt the new SDK; there is no reason to preserve any of the
  old SDK surface.

  ```bash
  # After git merge stops with conflicts:
  git checkout --theirs \
      crates/omnifs-sdk \
      crates/omnifs-sdk-macros \
      crates/omnifs-mount-schema \
      crates/host \
      crates/cli \
      wit/provider.wit
  git add crates/omnifs-sdk crates/omnifs-sdk-macros crates/omnifs-mount-schema \
          crates/host crates/cli wit/provider.wit
  ```

- For `Cargo.toml` at the workspace root: merge by hand. The final
  `members` line must be:

  ```toml
  members = ["crates/*", "providers/github", "providers/dns", "providers/google-drive", "providers/test"]
  ```

  Take everything else from main.

- For `Cargo.lock`: delete and let `cargo` regenerate.

  ```bash
  rm Cargo.lock
  ```

- The provider's own `providers/google-drive/**` files remain as their
  old-SDK versions after the merge. They are about to be rewritten in
  section 6.

After resolving:

```bash
git commit
```

If the merge produces a dramatic number of trivial "keep main" wins,
consider the equivalent rebase workflow; either shape is acceptable as
long as the final tree has main's SDK and the provider's source still
present (to be rewritten).

## 6. Per-file migration

The rewrite removes `tree.rs` entirely and replaces the dir/subtree
trait impls with free-function `#[handlers]`. `api.rs` keeps the Drive
client and `FileMeta` logic minus `EntryStat`/`Entry` helpers (the new
SDK builds projections, not `Entry`s). `http_ext.rs` keeps its thin
bearer-token + Accept extension and drops the now-empty `State` import
(`State` still exists; it just has no fields the helper reads).

### 6.1 `providers/google-drive/src/lib.rs` (rewrite)

Replace with:

```rust
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

pub(crate) use omnifs_sdk::prelude::Result;

mod api;
mod http_ext;
mod items;
mod provider;
mod root;

#[derive(Clone)]
pub(crate) struct State {
    pub(crate) oauth_access_token: String,
    pub(crate) page_size: u32,
}

#[omnifs_sdk::config]
struct Config {
    #[serde(default)]
    oauth_access_token: String,
    #[serde(default = "default_page_size")]
    page_size: u32,
}

fn default_page_size() -> u32 {
    1000
}
```

Notes:

- `oauth_access_token` is read on every request. It enters state at
  `init` time; refreshing it requires provider reload (the host does
  not yet have an OAuth refresh callout). If the config omits the
  token, requests will 401; the old provider did not enforce this
  either. A future `on_event`-driven refresh is a follow-up.
- `page_size` defaults to 1000 to match the old client's hardcoded
  `pageSize=1000`.

### 6.2 `providers/google-drive/src/provider.rs` (rewrite)

Replace with:

```rust
use omnifs_sdk::prelude::*;

use crate::{Config, State};

#[provider(mounts(crate::root::RootHandlers, crate::items::ItemHandlers))]
impl GoogleDriveProvider {
    fn init(config: Config) -> Result<(State, ProviderInfo)> {
        Ok((
            State {
                oauth_access_token: config.oauth_access_token,
                page_size: config.page_size,
            },
            ProviderInfo {
                name: "google-drive-provider".to_string(),
                version: "0.1.0".to_string(),
                description: "Google Drive API provider for omnifs".to_string(),
            },
        ))
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["www.googleapis.com".to_string()],
            auth_types: vec!["bearer-token".to_string()],
            max_memory_mb: 64,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 0,
        }
    }
}
```

- No `on_event` handler is wired; the old provider had no invalidation
  emissions. If you decide to add periodic change-feed polling, model
  it after `providers/github/src/events.rs` (`cx.active_paths(...)`,
  `join_all`, `EventOutcome::invalidate_prefix`).

### 6.3 `providers/google-drive/src/http_ext.rs` (rewrite)

Replace with:

```rust
use omnifs_sdk::Cx;
use omnifs_sdk::http::Request;

use crate::State;

pub(crate) trait DriveHttpExt {
    fn drive_get(&self, url: impl Into<String>) -> Request<'_, State>;
    fn drive_json(&self, url: impl Into<String>) -> Request<'_, State>;
}

impl DriveHttpExt for Cx<State> {
    fn drive_get(&self, url: impl Into<String>) -> Request<'_, State> {
        let token = self.state(|state| state.oauth_access_token.clone());
        let mut req = self.http().get(url);
        if !token.is_empty() {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        req
    }

    fn drive_json(&self, url: impl Into<String>) -> Request<'_, State> {
        self.drive_get(url).header("Accept", "application/json")
    }
}
```

Only change vs. the old file: the `Authorization: Bearer` header is
now attached in `drive_get` instead of being assumed to come from the
host's ambient bearer-token store. The new SDK does not auto-inject
auth headers; the `auth_types: vec!["bearer-token"]` capability is
advisory.

### 6.4 `providers/google-drive/src/api.rs` (rewrite)

The client itself, export-format tables, and the `folder_children`
disambiguation code are kept. The `EntryStat`/`Entry` methods
(`entry()`, `entry_stat()`) and the `omnifs_sdk::mount::*` imports are
removed; the handlers build `Projection`s directly and use a small
helper `file_size_from_meta` that returns an `Option<NonZeroU64>`
suitable for `FileStat`.

Replace the contents with:

```rust
use core::num::NonZeroU64;
use std::collections::{BTreeMap, BTreeSet};

use omnifs_sdk::Cx;
use omnifs_sdk::prelude::*;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use url::Url;

use crate::http_ext::DriveHttpExt;
use crate::{Result, State};

pub(crate) const API_BASE: &str = "https://www.googleapis.com/drive/v3";

const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const DOCS_MIME: &str = "application/vnd.google-apps.document";
const SHEETS_MIME: &str = "application/vnd.google-apps.spreadsheet";
const SLIDES_MIME: &str = "application/vnd.google-apps.presentation";

const FILE_LIST_FIELDS: &str =
    "nextPageToken,files(id,name,mimeType,size,parents,modifiedTime,resourceKey)";
const FILE_GET_FIELDS: &str = "id,name,mimeType,size,parents,modifiedTime,resourceKey";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileMeta {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) name: String,
    pub(crate) mime_type: String,
    #[serde(default)]
    pub(crate) size: Option<String>,
    #[serde(default)]
    pub(crate) parents: Vec<String>,
    #[serde(default)]
    pub(crate) modified_time: Option<String>,
    #[serde(default)]
    pub(crate) resource_key: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NodeKind {
    Folder,
    Blob,
    WorkspaceDoc(WorkspaceKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkspaceKind {
    Document,
    Spreadsheet,
    Presentation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ExportFormat {
    pub(crate) name: &'static str,
    pub(crate) mime_type: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FolderChild {
    pub(crate) display_name: String,
    pub(crate) meta: FileMeta,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListFilesResponse {
    #[serde(default)]
    files: Vec<FileMeta>,
    #[serde(default)]
    next_page_token: Option<String>,
}

pub(crate) struct DriveApi<'a> {
    cx: &'a Cx<State>,
}

impl<'a> DriveApi<'a> {
    pub(crate) fn new(cx: &'a Cx<State>) -> Self {
        Self { cx }
    }

    pub(crate) async fn list_folder_children(
        &self,
        folder_id: &str,
    ) -> Result<Vec<FileMeta>> {
        let page_size = self
            .cx
            .state(|state| state.page_size.to_string());
        let mut items = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let body = self
                .cx
                .drive_json(self.list_files_url(folder_id, &page_size, page_token.as_deref())?)
                .send_body()
                .await?;
            let page: ListFilesResponse = parse_model(&body)?;
            items.extend(page.files);

            let Some(next_page_token) = page.next_page_token else {
                return Ok(items);
            };
            page_token = Some(next_page_token);
        }
    }

    pub(crate) async fn get_file(&self, file_id: &str) -> Result<FileMeta> {
        let body = self
            .cx
            .drive_json(self.file_url(file_id, &[("fields", FILE_GET_FIELDS)])?)
            .send_body()
            .await?;
        parse_model(&body)
    }

    pub(crate) async fn read_blob(&self, file_id: &str) -> Result<Vec<u8>> {
        self.cx
            .drive_get(self.file_url(file_id, &[("alt", "media")])?)
            .send_body()
            .await
    }

    pub(crate) async fn export(&self, file_id: &str, mime_type: &str) -> Result<Vec<u8>> {
        self.cx
            .drive_get(self.export_url(file_id, mime_type)?)
            .send_body()
            .await
    }

    fn list_files_url(
        &self,
        folder_id: &str,
        page_size: &str,
        page_token: Option<&str>,
    ) -> Result<String> {
        let query = format!("'{folder_id}' in parents and trashed=false");
        let mut pairs = vec![
            ("corpora", "user"),
            ("fields", FILE_LIST_FIELDS),
            ("pageSize", page_size),
            ("q", query.as_str()),
            ("spaces", "drive"),
            ("supportsAllDrives", "true"),
        ];
        if let Some(page_token) = page_token {
            pairs.push(("pageToken", page_token));
        }
        build_url(&format!("{API_BASE}/files"), &pairs)
    }

    fn file_url(&self, file_id: &str, extra_pairs: &[(&str, &str)]) -> Result<String> {
        let mut pairs = vec![("supportsAllDrives", "true")];
        pairs.extend_from_slice(extra_pairs);
        build_url(&format!("{API_BASE}/files/{file_id}"), &pairs)
    }

    fn export_url(&self, file_id: &str, mime_type: &str) -> Result<String> {
        build_url(
            &format!("{API_BASE}/files/{file_id}/export"),
            &[("mimeType", mime_type), ("supportsAllDrives", "true")],
        )
    }
}

impl FileMeta {
    pub(crate) fn node_kind(&self) -> NodeKind {
        if self.mime_type == FOLDER_MIME {
            NodeKind::Folder
        } else if let Some(kind) = WorkspaceKind::for_mime_type(&self.mime_type) {
            NodeKind::WorkspaceDoc(kind)
        } else {
            NodeKind::Blob
        }
    }

    pub(crate) fn display_name_base(&self) -> String {
        let mut name = sanitize_segment(&self.name);
        if let NodeKind::WorkspaceDoc(kind) = self.node_kind() {
            name.push_str(kind.suffix());
        }
        name
    }

    pub(crate) fn file_stat(&self) -> Option<FileStat> {
        self.size
            .as_deref()
            .and_then(|value| value.parse::<u64>().ok())
            .and_then(NonZeroU64::new)
            .map(|size| FileStat { size })
    }
}

impl WorkspaceKind {
    fn for_mime_type(mime_type: &str) -> Option<Self> {
        match mime_type {
            DOCS_MIME => Some(Self::Document),
            SHEETS_MIME => Some(Self::Spreadsheet),
            SLIDES_MIME => Some(Self::Presentation),
            _ => None,
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Self::Document => ".gdoc",
            Self::Spreadsheet => ".gsheet",
            Self::Presentation => ".gslides",
        }
    }

    fn export_formats(self) -> &'static [ExportFormat] {
        match self {
            Self::Document => &[
                ExportFormat {
                    name: "pdf",
                    mime_type: "application/pdf",
                },
                ExportFormat {
                    name: "docx",
                    mime_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                },
                ExportFormat {
                    name: "txt",
                    mime_type: "text/plain",
                },
            ],
            Self::Spreadsheet => &[
                ExportFormat {
                    name: "pdf",
                    mime_type: "application/pdf",
                },
                ExportFormat {
                    name: "xlsx",
                    mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                },
            ],
            Self::Presentation => &[
                ExportFormat {
                    name: "pdf",
                    mime_type: "application/pdf",
                },
                ExportFormat {
                    name: "pptx",
                    mime_type: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
                },
            ],
        }
    }
}

pub(crate) fn folder_children(metas: Vec<FileMeta>) -> Vec<FolderChild> {
    let base_names = metas
        .iter()
        .map(FileMeta::display_name_base)
        .collect::<Vec<_>>();

    let mut groups = BTreeMap::<String, Vec<usize>>::new();
    for (index, base_name) in base_names.iter().enumerate() {
        groups.entry(base_name.clone()).or_default().push(index);
    }

    let mut names = base_names.clone();
    for (base_name, indexes) in groups {
        if indexes.len() < 2 {
            continue;
        }

        let short_ids = indexes
            .iter()
            .map(|index| short_id(&metas[*index].id))
            .collect::<Vec<_>>();
        let short_ids_unique = short_ids.iter().collect::<BTreeSet<_>>().len() == short_ids.len();

        for (position, index) in indexes.into_iter().enumerate() {
            let suffix = if short_ids_unique {
                short_ids[position].clone()
            } else {
                metas[index].id.clone()
            };
            names[index] = format!("{base_name} [{suffix}]");
        }
    }

    let mut children = metas
        .into_iter()
        .zip(names)
        .map(|(meta, display_name)| FolderChild { display_name, meta })
        .collect::<Vec<_>>();
    children.sort_by(|left, right| {
        sort_key(&left.meta, &left.display_name).cmp(&sort_key(&right.meta, &right.display_name))
    });
    children
}

pub(crate) fn export_format_named(meta: &FileMeta, name: &str) -> Option<ExportFormat> {
    let NodeKind::WorkspaceDoc(kind) = meta.node_kind() else {
        return None;
    };
    kind.export_formats()
        .iter()
        .copied()
        .find(|format| format.name == name)
}

pub(crate) fn export_formats(meta: &FileMeta) -> &'static [ExportFormat] {
    let NodeKind::WorkspaceDoc(kind) = meta.node_kind() else {
        return &[];
    };
    kind.export_formats()
}

fn build_url(base: &str, pairs: &[(&str, &str)]) -> Result<String> {
    let mut url = Url::parse(base)
        .map_err(|error| ProviderError::internal(format!("invalid API URL {base}: {error}")))?;
    {
        let mut query = url.query_pairs_mut();
        for (key, value) in pairs {
            query.append_pair(key, value);
        }
    }
    Ok(url.into())
}

fn parse_model<T>(body: &[u8]) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_slice(body)
        .map_err(|error| ProviderError::invalid_input(format!("JSON parse error: {error}")))
}

fn sanitize_segment(name: &str) -> String {
    let sanitized = name.replace('/', "_");
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        "unnamed".to_string()
    } else {
        sanitized
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect::<String>()
}

fn sort_key(meta: &FileMeta, display_name: &str) -> (u8, String, String) {
    let kind_rank = match meta.node_kind() {
        NodeKind::Blob => 1,
        NodeKind::Folder | NodeKind::WorkspaceDoc(_) => 0,
    };
    (
        kind_rank,
        display_name.to_ascii_lowercase(),
        meta.id.clone(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(id: &str, name: &str) -> FileMeta {
        FileMeta {
            id: id.to_string(),
            name: name.to_string(),
            mime_type: "text/plain".to_string(),
            size: Some("12".to_string()),
            parents: Vec::new(),
            modified_time: None,
            resource_key: None,
        }
    }

    #[test]
    fn folder_children_disambiguate_duplicate_names() {
        let children = folder_children(vec![
            blob("abcd1234rest", "readme"),
            blob("wxyz9876rest", "readme"),
        ]);
        let names = children
            .into_iter()
            .map(|child| child.display_name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["readme [abcd1234]", "readme [wxyz9876]"]);
    }

    #[test]
    fn workspace_docs_use_suffixes() {
        let doc = FileMeta {
            id: "doc-1".to_string(),
            name: "Plan".to_string(),
            mime_type: DOCS_MIME.to_string(),
            size: None,
            parents: Vec::new(),
            modified_time: None,
            resource_key: None,
        };
        assert_eq!(doc.display_name_base(), "Plan.gdoc");
        assert_eq!(
            export_formats(&doc)
                .iter()
                .map(|format| format.name)
                .collect::<Vec<_>>(),
            vec!["pdf", "docx", "txt"]
        );
    }
}
```

### 6.5 `providers/google-drive/src/root.rs` (new file)

```rust
use omnifs_sdk::prelude::*;

use crate::api::{DriveApi, NodeKind, folder_children};
use crate::{Result, State};

const MY_DRIVE_ROOT_ID: &str = "root";

pub struct RootHandlers;

#[handlers]
impl RootHandlers {
    #[dir("/")]
    fn root(_cx: &DirCx<'_, State>) -> Result<Projection> {
        let mut p = Projection::new();
        p.dir("my-drive");
        p.dir("_items");
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/my-drive")]
    async fn my_drive(cx: &DirCx<'_, State>) -> Result<Projection> {
        project_folder(cx, MY_DRIVE_ROOT_ID).await
    }

    #[dir("/my-drive/{name}")]
    async fn my_drive_child(cx: &DirCx<'_, State>, name: String) -> Result<Projection> {
        project_named_child(cx, MY_DRIVE_ROOT_ID, &name).await
    }
}

async fn project_folder(cx: &Cx<State>, folder_id: &str) -> Result<Projection> {
    let api = DriveApi::new(cx);
    let children = folder_children(api.list_folder_children(folder_id).await?);

    let mut projection = Projection::new();
    for child in &children {
        match child.meta.node_kind() {
            NodeKind::Folder | NodeKind::WorkspaceDoc(_) => {
                projection.dir(&child.display_name);
            },
            NodeKind::Blob => {
                if let Some(stat) = child.meta.file_stat() {
                    projection.file_with_stat(&child.display_name, stat);
                } else {
                    projection.file(&child.display_name);
                }
            },
        }
    }
    projection.page(PageStatus::Exhaustive);
    Ok(projection)
}

async fn project_named_child(
    cx: &Cx<State>,
    parent_id: &str,
    name: &str,
) -> Result<Projection> {
    let api = DriveApi::new(cx);
    let children = folder_children(api.list_folder_children(parent_id).await?);
    let Some(child) = children.into_iter().find(|child| child.display_name == name) else {
        return Err(ProviderError::not_found(format!("path not found: {name}")));
    };

    match child.meta.node_kind() {
        NodeKind::Folder => project_folder(cx, &child.meta.id).await,
        NodeKind::WorkspaceDoc(_) => {
            let mut projection = Projection::new();
            for format in crate::api::export_formats(&child.meta) {
                projection.file(format.name);
            }
            projection.page(PageStatus::Exhaustive);
            Ok(projection)
        },
        NodeKind::Blob => {
            let mut projection = Projection::new();
            match child.meta.file_stat() {
                Some(stat) => projection.file_with_stat("content", stat),
                None => projection.file("content"),
            }
            projection.page(PageStatus::Exhaustive);
            Ok(projection)
        },
    }
}
```

Preload/sibling notes for `/my-drive`:

- The old provider did not preload blob bytes at listing time. The new
  provider does not either, because listings can include large blobs
  that exceed the 64 KiB eager budget. If you want an opt-in optimization
  for small blobs, gate it on `meta.size` < 32 KiB and call
  `projection.preload(format!("/my-drive/{display_name}"), bytes)` after
  fetching via `DriveApi::read_blob`. This is a follow-up; do not add
  it unless asked.

### 6.6 `providers/google-drive/src/items.rs` (new file)

```rust
use omnifs_sdk::prelude::*;

use crate::api::{DriveApi, ExportFormat, FileMeta, NodeKind, export_format_named, export_formats};
use crate::{Result, State};

pub struct ItemHandlers;

#[handlers]
impl ItemHandlers {
    #[dir("/_items")]
    fn items_root(_cx: &DirCx<'_, State>) -> Result<Projection> {
        // Dynamic: ids are not enumerable. Declare partial so static
        // siblings don't claim exhaustiveness, and leave entries empty.
        let mut projection = Projection::new();
        projection.page(PageStatus::More(Cursor::Opaque("dynamic".to_string())));
        Ok(projection)
    }

    #[dir("/_items/{file_id}")]
    async fn item_root(cx: &DirCx<'_, State>, file_id: String) -> Result<Projection> {
        let meta = DriveApi::new(cx).get_file(&file_id).await?;
        let meta_json = encode_meta(&meta)?;

        let mut projection = Projection::new();
        projection.file_with_content("meta.json", meta_json);
        match meta.node_kind() {
            NodeKind::Blob => match meta.file_stat() {
                Some(stat) => projection.file_with_stat("content", stat),
                None => projection.file("content"),
            },
            NodeKind::WorkspaceDoc(_) => projection.dir("exports"),
            NodeKind::Folder => {},
        }
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[file("/_items/{file_id}/meta.json")]
    async fn item_meta(cx: &Cx<State>, file_id: String) -> Result<FileContent> {
        let meta = DriveApi::new(cx).get_file(&file_id).await?;
        Ok(FileContent::bytes(encode_meta(&meta)?))
    }

    #[file("/_items/{file_id}/content")]
    async fn item_content(cx: &Cx<State>, file_id: String) -> Result<FileContent> {
        let api = DriveApi::new(cx);
        let meta = api.get_file(&file_id).await?;
        match meta.node_kind() {
            NodeKind::Blob => {
                let bytes = api.read_blob(&meta.id).await?;
                Ok(FileContent::bytes(bytes))
            },
            NodeKind::Folder | NodeKind::WorkspaceDoc(_) => {
                Err(ProviderError::not_a_file("content is only available for blob files"))
            },
        }
    }

    #[dir("/_items/{file_id}/exports")]
    async fn item_exports(cx: &DirCx<'_, State>, file_id: String) -> Result<Projection> {
        let meta = DriveApi::new(cx).get_file(&file_id).await?;
        let formats = export_formats(&meta);
        if formats.is_empty() {
            return Err(ProviderError::not_found(
                "exports are only available for workspace docs".to_string(),
            ));
        }
        let mut projection = Projection::new();
        for format in formats {
            projection.file(format.name);
        }
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[file("/_items/{file_id}/exports/{format}")]
    async fn item_export(
        cx: &Cx<State>,
        file_id: String,
        format: String,
    ) -> Result<FileContent> {
        let api = DriveApi::new(cx);
        let meta = api.get_file(&file_id).await?;
        let Some(ExportFormat { mime_type, .. }) = export_format_named(&meta, &format) else {
            return Err(ProviderError::not_found(format!(
                "unknown export format {format}"
            )));
        };
        let bytes = api.export(&meta.id, mime_type).await?;
        Ok(FileContent::bytes(bytes))
    }
}

fn encode_meta(meta: &FileMeta) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(meta)
        .map_err(|error| ProviderError::internal(format!("failed to encode metadata JSON: {error}")))
}
```

Preload/sibling notes for `/_items/{file_id}`:

- `item_root` returns `meta.json` via `file_with_content(...)`. That
  means a later `read` of `/_items/{id}/meta.json` is served out of
  the host cache with no extra Drive call. This is the new SDK's
  native analogue of the old `ItemViewNode::Root` materialization.
- If you want the blob `content` to also be eager when small, gate it
  on `meta.size < 32 KiB` and read it in `item_root` with
  `api.read_blob(...)`, then call
  `projection.file_with_content("content", bytes)`. This is a
  follow-up; the default keeps `content` lazy so the 64 KiB budget is
  never at risk.

### 6.7 `providers/google-drive/src/tree.rs` (delete)

```bash
git rm providers/google-drive/src/tree.rs
```

All of `tree.rs`'s logic is redistributed:

- `Root` / `ItemsRoot` empty `Dir` impls → `root::RootHandlers::root` and
  `items::ItemHandlers::items_root`.
- `MyDriveTree` `Subtree` impl → `root::my_drive` and
  `root::my_drive_child` (single-level path resolution).
- `ItemTree` `Subtree` impl → `items::item_root`, `items::item_meta`,
  `items::item_content`, `items::item_exports`, `items::item_export`.
- `resolve_drive_tree` / `resolve_item_view` state machines disappear:
  the new SDK's path-first dispatch replaces them.
- `DriveTreeNode` / `ItemViewNode` enums disappear.

## 7. Event handling migration

Old SDK pattern (from the pre-merge `CLAUDE.md`): providers emit
`Effect::CacheInvalidatePrefix` from inside handlers to invalidate
cached listings. This exact path is gone; the new SDK separates event
handling from request handling.

| Old | New |
|---|---|
| `Effect::CacheInvalidatePrefix` returned from a handler | Not supported. Handlers return only `Projection` / `FileContent` / `SubtreeRef`. |
| `Effect::CacheInvalidateIdentity` / `Scope` | Removed entirely. |
| Periodic polling via provider-owned timer | `on_event(ProviderEvent::TimerTick, ...)` handler, scheduled by `refresh_interval_secs` in capabilities. |

Current state in this provider: the old code never emitted
cache-invalidate effects, so nothing needs porting. The provider.rs in
section 6.2 omits `on_event` entirely. If a change-feed polling
mechanism is added later (Drive has `changes.list`), model it after
`providers/github/src/events.rs`:

```rust
async fn on_event(cx: Cx<State>, event: ProviderEvent) -> Result<EventOutcome> {
    let mut outcome = EventOutcome::new();
    if let ProviderEvent::TimerTick(_) = event {
        // Poll Drive changes.list for each active mount, collect the
        // affected /_items/{id} paths and /my-drive subpaths, then:
        // outcome.invalidate_prefix("/my-drive");
        // outcome.invalidate_path(format!("/_items/{id}"));
    }
    Ok(outcome)
}
```

Also set `refresh_interval_secs: 60` (or whatever the polling cadence
is) in `capabilities()` if you wire this up. Leave it at `0` otherwise.

## 8. Cargo.toml changes

### 8.1 Provider crate: `providers/google-drive/Cargo.toml`

Current file:

```toml
[package]
name = "omnifs-provider-google-drive"
version = "0.1.0"
edition = "2024"
description = "OmnIFS provider for Google Drive browsing"
license = "MIT OR Apache-2.0"
repository = "https://github.com/raulk/omnifs"
homepage = "https://github.com/raulk/omnifs"

[lib]
crate-type = ["cdylib", "lib"]

[dependencies]
omnifs-sdk = { path = "../../crates/omnifs-sdk" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
url = "2"

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

Keep the file as-is. `omnifs-sdk` already re-exports `serde_json`,
`serde`, and `hashbrown`, but the provider uses `url` directly and
uses `serde_json::to_vec_pretty` / `serde_json::from_slice` at call
sites that import `serde_json` by its crate name, so the declared
dependencies are still necessary.

### 8.2 Workspace root `Cargo.toml`

After the merge (section 5), the final workspace root `Cargo.toml`
`members` line must include `"providers/google-drive"`:

```toml
[workspace]
resolver = "2"
members = ["crates/*", "providers/github", "providers/dns", "providers/google-drive", "providers/test"]
default-members = ["crates/cli", "crates/host"]
```

Main's own `Cargo.toml` at `6343486` reads:

```toml
members = ["crates/*", "providers/github", "providers/dns", "providers/test"]
```

The merge must end with the `google-drive` entry added (it was already
present in the pre-merge worktree, so this is typically resolved by
preferring the worktree side for that one line).

## 9. Verification

Run each command from the worktree root. The first one is the
definitive post-migration gate.

```bash
# 1. Format check. Run once at the start and after the rewrite.
cargo fmt --check

# 2. Provider-side lint (wasm32-wasip2 target, deny warnings).
cargo clippy -p omnifs-provider-google-drive --target wasm32-wasip2 -- -D warnings

# 3. Provider-side test compile (no-run; the host test harness cannot
#    execute wasm32-wasip2 binaries).
cargo test -p omnifs-provider-google-drive --target wasm32-wasip2 --no-run

# 4. Full provider sweep: clippy + test-compile for every wasm32-wasip2
#    provider. Must pass for all providers together.
just check-providers

# 5. Full native sweep (fmt + clippy + test for host crates, plus
#    check-providers). Final gate.
just check
```

Expected outcomes:

- `cargo fmt --check` passes with zero diff.
- Clippy passes with `-D warnings` against the wasm32-wasip2 target.
- `cargo test --no-run` produces test binaries but does not execute
  them.
- `just check` exercises host crates on the native target and
  re-invokes `just check-providers` for the wasm-side sweep.

## 10. Risks and gotchas

**OAuth tokens.** The old provider declared `auth_types:
["bearer-token"]` but never attached an `Authorization` header in the
code that survives in this plan. The new `http_ext.rs` in section 6.3
always attaches `Bearer {token}` when `state.oauth_access_token` is
non-empty. Tokens expire (typically one hour). There is no refresh
path in this migration; provider reload is the only way to rotate the
token today. Surface this as a follow-up if the user expects
long-lived mounts. Do not add a silent refresh handler without
explicit direction.

**Large binary files exceeding the 64 KiB eager budget.**
`Projection::file_with_content` and `Projection::preload` reject
payloads above 64 KiB (`handler::MAX_PROJECTED_BYTES`). The rewrite
never pushes `blob` bytes eagerly for that reason. Do not extend the
preload path to blobs without first gating on `meta.size`. Workspace
doc exports (PDF/DOCX/XLSX/PPTX) are almost always above the budget
too; they must stay lazy.

**Google Drive export MIME types.** `ExportFormat::mime_type` drives
the `mimeType` query param in `/files/{id}/export`. The set in
`WorkspaceKind::export_formats` is the authoritative list; do not
change it during the migration. Drive can reject unsupported
combinations (e.g. exporting a spreadsheet as `.docx`); the request
surfaces as an HTTP 4xx, which `send_body` maps to a
`ProviderError::InvalidInput` via `from_http_status`. This is the
correct terminal behavior; do not try to retry.

**Pagination tokens (`nextPageToken`).** The Drive `files.list` API
pages via `nextPageToken`. The new handler's listing is
`Page::exhaustive(...)`-equivalent (the rewrite uses
`projection.page(PageStatus::Exhaustive)` after draining all pages in
`DriveApi::list_folder_children`). That is the same shape as the old
provider: listings are exhaustive from the host's perspective, even
though they internally follow page tokens. If you switch to
per-page cursor handoff via `PageStatus::More(Cursor::Opaque(...))`,
the cursor must be opaque to the host and interpretable by the
handler on the next call; the current SDK does not carry the cursor
into the handler input, so per-request paging is a follow-up requiring
SDK work, not a drop-in change.

**Subtree handoff removal.** The old provider used
`(subtree)` mounts for `/my-drive` and `/_items/{file_id}`. The new
SDK's `#[subtree]` is strictly for git repositories (the handler
returns a `SubtreeRef { tree_ref }` from a `Callout::GitOpenRepo`).
Drive has no git; do not re-introduce `#[subtree]` handlers here.
Directory handlers cover the same surface.

**Host cache ownership.** Do not reintroduce `moka` / LRUs / TTLs in
the provider. The new SDK caches via the host; the only knobs a
provider has are `EventOutcome` invalidations from `on_event`.
Provider-side caches are explicitly forbidden by the project
`CLAUDE.md`.

**Non-zero file sizes.** Use `FileStat::placeholder()` (4096 bytes) or
`file_stat()` from `FileMeta` for blobs whose real size is known. The
new SDK enforces non-zero sizes via `NonZeroU64`; listings that set
size to zero would never be read. `Projection::file(name)` picks up
the placeholder size automatically.

**`hashbrown::HashMap` vs `std::collections::HashMap` in providers.**
Use `hashbrown` for provider-internal maps. It keeps provider
internals predictable across WASI targets. `omnifs_sdk` re-exports
`hashbrown` for generated code; add it as a direct dependency only if
provider code itself needs it (the current rewrite does not).

**Provider tests can't execute on wasm32-wasip2 directly in Cargo's
test harness.** Always use `--no-run` for target-specific compilation
checks. The `api.rs` unit tests (`folder_children_disambiguate_...`,
`workspace_docs_use_suffixes`) compile under the wasm target and are
meant to be executed on the native target; they do not use any
wasm-only SDK symbols, so `cargo test -p omnifs-provider-google-drive`
(no `--target`) will run them if you ever want to execute them.

---

## Verification

- `cargo fmt --check`
- `cargo clippy -p omnifs-provider-google-drive --target wasm32-wasip2 -- -D warnings`
- `cargo test -p omnifs-provider-google-drive --target wasm32-wasip2 --no-run`
- `just check-providers`

All must pass. If `just` is not on PATH, note that in the PR body and run
the equivalent `cargo` commands from the root of this branch's worktree.

## Commit

Conventional:

```
feat(google-drive): migrate provider to path-first handler SDK
```

Body: one paragraph naming the major structural changes and the base SDK
PRs (#28 `feat/sdk-http-post-support`, #29 `feat/sdk-path-rest-captures`,
#D `feat/sdk-error-constructors`).

## PR

- Title: `feat(google-drive): migrate provider to path-first handler SDK`
- Body: summary + link to this branch's `PLAN.md` + verification results
  (which cargo commands ran, which passed, which were skipped and why).
