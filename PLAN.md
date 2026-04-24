# feat/migrate-tailscale

The tailscale provider in this worktree (`.worktrees/providers/tailscale`, tip `e1d0b85`, fork point `7742e99`) was authored against the mount-table SDK that predates commit `6343486` ("refactor!: redesign provider SDK and host runtime around path-first handlers and callouts") on `main`.

## Blocked by

This plan cannot start execution until both of these have merged into `main`:

- PR #28 `feat/sdk-http-post-support` — https://github.com/raulk/omnifs/pull/28
- PR #29 `feat/sdk-path-rest-captures` — https://github.com/raulk/omnifs/pull/29

Note: `ProviderError::rate_limited` / `::permission_denied` / `::version_mismatch`
constructors are already on `main` (landed with the #27 refactor). No separate PR
is needed.

## Execution model

This branch was created off `main` at `6343486`. To execute:

1. `git -C /Users/raul/W/gvfs worktree add /Users/raul/W/gvfs/.worktrees/migrate-tailscale feat/migrate-tailscale`
2. Work in that worktree only.
3. Bring in the provider source from the old worktree at
   `/Users/raul/W/gvfs/.worktrees/providers/tailscale/providers/tailscale/`
   per the "Port provider source" step below.
4. Execute this PLAN.md end-to-end. Corrections in the "Migration
   corrections" section are authoritative over anything in the reference
   body that contradicts them.
5. Run the Verification commands listed near the bottom.
6. Commit on the `feat/migrate-tailscale` branch, push, open PR.


## Port provider source

This branch is off `main` at `6343486`, so there is NO merge from
`wip/provider-tailscale-impl` and NO `git merge main`. The wip branch carries OLD-SDK infrastructure
that must not land here. Only provider-local files come over, file by file,
using `git checkout <old-branch> -- <path>` (this pulls the file contents into
the working tree and index without touching anything else).

### Files to copy verbatim (no touch-ups beyond rust import paths / `ProviderResult` → `Result`)

- `providers/tailscale/src/types.rs`

Bring each over with:

```bash
git checkout wip/provider-tailscale-impl -- providers/tailscale/src/types.rs
```

### Files to copy then touch up

- `providers/tailscale/src/api.rs` — copy over, then apply the corrections in
  the relevant reference-body sections (auth removal, POST rewrites, error
  constructor substitutions).
- `providers/tailscale/src/nodes.rs` — copy over, then apply the corrections in
  the relevant reference-body sections (auth removal, POST rewrites, error
  constructor substitutions).

Bring them in with:

```bash
git checkout wip/provider-tailscale-impl -- providers/tailscale/src/api.rs
git checkout wip/provider-tailscale-impl -- providers/tailscale/src/nodes.rs
```

Then edit in place.

### Files to create fresh (do NOT copy from the wip branch)

- `providers/tailscale/src/lib.rs`
- `providers/tailscale/src/provider.rs`
- `providers/tailscale/src/root.rs`
- `providers/tailscale/src/handlers/ (devices, users, acls)`

### Files to DISCARD (do NOT bring to this branch)

- `providers/tailscale/src/http_ext.rs`
- `providers/tailscale/src/old provider.rs`
- `providers/tailscale/src/old lib.rs`

These are old-SDK artifacts (entity projections, tree walkers, routes tables,
manual http_ext wrappers for auth). The new SDK shape replaces them with
path-first handlers.

### Bring over the provider Cargo.toml

```bash
git checkout wip/provider-tailscale-impl -- providers/tailscale/Cargo.toml
```

Then update its SDK dependency declarations to match `providers/github/Cargo.toml`
on the current `main`. In particular, `omnifs-sdk` must point at the workspace
version and not an old path/git revision.

### Re-register the provider in the workspace

The workspace-level `Cargo.toml` on `main` dropped every non-dns/github/test
provider. Re-add `providers/tailscale` to its `members` array. Example diff:

```toml
[workspace]
members = [
    "crates/cli",
    "crates/host",
    "providers/dns",
    "providers/github",
+   "providers/tailscale",
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

Tailscale uses API-key auth. The host injects the required
`Authorization: Bearer <key>` header (tailscale's API accepts bearer
tokens interchangeably with API keys) via `AuthManager::headers_for_url`.

```rust
Capabilities {
    auth_types: vec!["bearer-token".to_string()],
    domains: vec!["api.tailscale.com".to_string()],
    ..Default::default()
}
```

Remove every `api_key` field on `Config` or `State` and every manual
`.header("Authorization", ...)` in `http_ext.rs` or handler code. Any
bullet in the original plan body that says "thread api_key through
State" is superseded.

Domains covered:

  - `api.tailscale.com`

Mount config shape:

```json
{
  "plugin": "tailscale.wasm",
  "mount": "/tailscale",
  "auth": [{"type": "bearer-token", "token_env": "TAILSCALE_API_KEY", "domain": "api.tailscale.com"}]
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
> `/Users/raul/W/gvfs/.worktrees/providers/tailscale/MIGRATION_PLAN.md`.
> Read it for provider-specific shape, path tables, gotchas, and per-file
> migration notes. Wherever a passage conflicts with the corrections above
> (auth handling, POST shape, error constructors, rest captures, destructive
> action for crates-io), the corrections win.

# Tailscale provider migration plan

## Summary

The tailscale provider in this worktree (`.worktrees/providers/tailscale`,
tip `e1d0b85`, fork point `7742e99`) was authored against the mount-table
SDK that predates commit `6343486` ("refactor!: redesign provider SDK and
host runtime around path-first handlers and callouts") on `main`. The
provider must be migrated to the new path-first handlers SDK, which:

- replaces the `mounts! { ... }` DSL plus `Dir`/`File`/`Subtree` trait
  implementations with free-function handlers annotated
  `#[omnifs_sdk::dir(..)]` / `#[file(..)]` / `#[subtree(..)]` grouped under
  a handlers struct annotated `#[handlers]`
- replaces `Projection<'_, Path>` with owned `Projection`
- removes `materialize()` entirely (the host calls `lookup_child`,
  `list_children`, `read_file` via the registry built by the handler
  macros)
- replaces the free-standing `__mounts::{RootPath, DevicePath, ...}`
  typestate with typed captures in the handler signatures
  (`device_id: NodeId`, `user_id: UserId`)
- replaces `Effect`/`SingleEffect`/`CacheInvalidate*` with request/response
  `Callout`s (HTTP + git) and an `EventOutcome` returned from `on_event`
- replaces the separate `ProviderResult<T> = Result<T, ProviderError>`
  with the SDK's `omnifs_sdk::prelude::Result` (same shape)

The migration preserves:

- all Tailscale API client logic in `api.rs` (verbatim)
- `NodeId` / `UserId` newtypes in `types.rs` (verbatim, still used as
  typed captures)
- `http_ext.rs` (refreshed to use an `auth_header` derived from
  `api_key`)
- the directory shape (`/_devices`, `/_devices/by-node`,
  `/_devices/by-node/{node_id}`, `/_users`, `/_users/by-id`,
  `/_users/by-id/{user_id}`, `/_policy`, `/_dns`)
- the preload idiom where listing `/_devices/by-node` warms the per-
  device files for every listed `{node_id}`, and similarly for users

This document is self-contained. Every replacement file is inlined in
full under "Per-file migration"; there is no need to consult any other
document to execute the plan.

## Current path table (verbatim from old `mounts!`)

```
capture node_id: crate::types::NodeId;
capture user_id: crate::types::UserId;

"/"                                (dir) => Root;
"/_devices"                        (dir) => Devices;
"/_devices/by-node"                (dir) => DevicesByNode;
"/_devices/by-node/{node_id}"      (dir) => Device;
"/_users"                          (dir) => Users;
"/_users/by-id"                    (dir) => UsersById;
"/_users/by-id/{user_id}"          (dir) => User;
"/_policy"                         (dir) => Policy;
"/_dns"                            (dir) => Dns;
```

## Target path table (new SDK, path-first)

All handlers live on one struct `RootHandlers` declared in
`src/root.rs`. `/_devices`, `/_users`, `/_policy`, `/_dns` are static
children of `/`, so they do not need their own handlers; the SDK
auto-derives sibling shape from declared child handlers. We still need
explicit `#[dir]` handlers for `/_devices/by-node`, `/_users/by-id`, and
the terminal `/_policy` / `/_dns` dirs because those are the ones that
call the API and project files.

```
#[dir("/")]                                  root
#[dir("/_devices")]                          devices_root        // static: exposes "by-node"
#[dir("/_devices/by-node")]                  devices_by_node     // API + preload per-device files
#[dir("/_devices/by-node/{node_id}")]        device              // API + project per-device files
#[dir("/_users")]                            users_root          // static: exposes "by-id"
#[dir("/_users/by-id")]                      users_by_id         // API + preload per-user files
#[dir("/_users/by-id/{user_id}")]            user                // API + project per-user files
#[dir("/_policy")]                           policy              // API + project acl.hujson, etag
#[dir("/_dns")]                              dns                 // API + project 4 files
```

Note: under the new SDK, static sibling shape of a parent is derived
from declared handlers. `/_devices` and `/_users` are pure static
passthroughs; we still need an explicit handler so that the parent `/`
knows these names exist (the `#[dir]` on each provides the static child
entry). Alternatively `/_devices` and `/_users` would be auto-derived
from the fact that `/_devices/by-node` and `/_users/by-id` are declared,
since the SDK walks the hierarchy. To stay close to the reference
providers we declare each intermediate directory explicitly.

## SDK cheatsheet (inlined verbatim)

### Provider shell (`lib.rs`)

```rust
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

pub(crate) use omnifs_sdk::prelude::Result;

mod api;
mod http_ext;
mod provider;
mod root;
pub(crate) mod types;

#[derive(Clone)]
pub(crate) struct State {
    pub(crate) tailnet: String,
    pub(crate) auth_header: String,
}

#[omnifs_sdk::config]
pub struct Config {
    pub tailnet: String,
    pub api_key: String,
}
```

### Provider impl (`provider.rs`)

```rust
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use omnifs_sdk::prelude::*;

use crate::{Config, State};

#[provider(mounts(crate::root::RootHandlers))]
impl TailscaleProvider {
    fn init(config: Config) -> Result<(State, ProviderInfo)> {
        let tailnet = config.tailnet.trim().to_string();
        if tailnet.is_empty() {
            return Err(ProviderError::invalid_input(
                "tailnet must not be empty".to_string(),
            ));
        }

        let api_key = config.api_key.trim().to_string();
        if api_key.is_empty() {
            return Err(ProviderError::invalid_input(
                "api_key must not be empty".to_string(),
            ));
        }

        let auth_header = format!("Basic {}", STANDARD.encode(format!("{api_key}:")));

        Ok((
            State { tailnet, auth_header },
            ProviderInfo {
                name: "tailscale-provider".to_string(),
                version: "0.1.0".to_string(),
                description: "Tailscale admin API provider for omnifs".to_string(),
            },
        ))
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["api.tailscale.com".to_string()],
            auth_types: vec![],
            max_memory_mb: 32,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 0,
        }
    }
}
```

### Handler API surface (inlined verbatim)

- `#[omnifs_sdk::dir("/segment/{cap}")]` on `fn` inside an `#[handlers] impl`.
- Handlers may be sync or async. Signature convention:
  - dir: `fn name(cx: &DirCx<'_, State>, caps...) -> Result<Projection>`
  - file: `fn name(cx: &Cx<State>, caps...) -> Result<FileContent>`
  - subtree: `fn name(cx: &Cx<State>, caps...) -> Result<SubtreeRef>`
- Typed captures: capture type must implement `FromStr` (our `NodeId`/
  `UserId` already do). Capture name in path (`{device_id}`) must match
  the parameter name.
- `DirCx<'_, S>` derefs to `Cx<S>`; use `cx.state(..)`, `cx.http()`,
  `cx.state_mut(..)`, `join_all(..)` inside handlers.
- `Projection` (owned):
  - `.dir(name)` / `.file(name)` — static stub child
  - `.file_with_stat(name, FileStat { size })` — project a file with a
    specific non-zero size
  - `.file_with_content(name, bytes)` — eager bytes (host caches; max 64
    KiB; automatically sized from `bytes.len()` with placeholder fallback)
  - `.page(PageStatus::Exhaustive)` — mark listing complete
  - `.page(PageStatus::More(Cursor::Opaque("...".into())))` — mark as
    continued
  - `.preload(path, bytes)` / `.preload_many(iter)` — hand file content
    to host for paths anywhere in the subtree (typical use: parent
    listing warms sibling file caches for children not served by this
    listing directly)
- `FileContent::bytes(bytes)` — non-streamed file content.
- `ProviderError::{not_found, invalid_input, internal,
  not_a_directory, not_a_file, unimplemented}` all take `impl
  Into<String>`.
- `Result<T>` is `omnifs_sdk::prelude::Result<T>` = `std::result::Result<
  T, ProviderError>`.
- `EventOutcome`: constructed with `EventOutcome::new()`; mutate with
  `.invalidate_path(path)` and `.invalidate_prefix(prefix)`; returned by
  `on_event`.

## Bringing the worktree up to main

The new SDK, WIT interface, and host runtime live on `main`. Pull them
by merging, taking main's copies wherever they conflict.

```bash
cd /Users/raul/W/gvfs/.worktrees/providers/tailscale

# Merge main into the worktree branch.
git fetch origin main
git merge origin/main
# Expected conflicts: crates/omnifs-sdk/**, crates/omnifs-sdk-macros/**,
# crates/host/**, crates/cli/**, wit/**, justfile, rust-toolchain.toml,
# root Cargo.toml (workspace members).

# Take main's versions everywhere in crates/ and wit/:
git checkout --theirs -- crates wit

# Root Cargo.toml needs hand-editing to re-add the tailscale member
# after taking main's version; see "Cargo.toml changes" below.
git checkout --theirs -- Cargo.toml
# then edit Cargo.toml to add "providers/tailscale" to workspace.members

# Keep the tailscale provider code from the worktree. It will not compile
# until the per-file migration below is applied, but we want to keep the
# files so we can rewrite them in place.
git checkout --ours -- providers/tailscale

git add crates wit Cargo.toml providers/tailscale
git commit -m "chore(tailscale): merge main and align to path-first SDK"
```

After the merge commit the worktree builds main's host + dns + github +
test providers cleanly. Only the tailscale provider is broken at this
point; the per-file migration below fixes it.

## Per-file migration

### `providers/tailscale/src/lib.rs` — rewrite

Old: declares `api`, `http_ext`, `nodes`, `provider`, `types` modules;
`mounts!` DSL; re-exports the handler structs from `nodes`. Delete the
`nodes` module and the `mounts!` block. Keep module declarations and
the `Config` / `State` / `ProviderResult` declarations, converted to
use `auth_header` and the new `Result` import.

New file (full replacement):

```rust
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

pub(crate) use omnifs_sdk::prelude::Result;

mod api;
mod http_ext;
mod provider;
mod root;
pub(crate) mod types;

#[derive(Clone)]
pub(crate) struct State {
    pub(crate) tailnet: String,
    pub(crate) auth_header: String,
}

#[omnifs_sdk::config]
pub struct Config {
    pub tailnet: String,
    pub api_key: String,
}
```

Notes:

- The old `ProviderResult<T>` alias is removed; use the re-exported
  `Result` from the prelude (same shape: `Result<T, ProviderError>`).
- `Config` is public because the `#[provider]` macro references it from
  the generated WIT glue, matching what the github and dns providers do.

### `providers/tailscale/src/provider.rs` — rewrite

Old: uses `omnifs_sdk::provider` (not `omnifs_sdk::prelude`), imports
`crate::__mounts`, returns `Result<(State, ProviderInfo), ProviderError>`.
New shape: `#[provider(mounts(crate::root::RootHandlers))]`; init returns
`Result<(State, ProviderInfo)>`; no `__mounts` import; capabilities
unchanged; add an `on_event` that invalidates the dynamic parts of the
tree on `TimerTick` (none, for now, so we leave it off and rely on
capacity eviction, matching the old behavior).

New file (full replacement):

```rust
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use omnifs_sdk::prelude::*;

use crate::{Config, State};

#[provider(mounts(crate::root::RootHandlers))]
impl TailscaleProvider {
    fn init(config: Config) -> Result<(State, ProviderInfo)> {
        let tailnet = config.tailnet.trim().to_string();
        if tailnet.is_empty() {
            return Err(ProviderError::invalid_input(
                "tailnet must not be empty".to_string(),
            ));
        }

        let api_key = config.api_key.trim().to_string();
        if api_key.is_empty() {
            return Err(ProviderError::invalid_input(
                "api_key must not be empty".to_string(),
            ));
        }

        let auth_header = format!("Basic {}", STANDARD.encode(format!("{api_key}:")));

        Ok((
            State { tailnet, auth_header },
            ProviderInfo {
                name: "tailscale-provider".to_string(),
                version: "0.1.0".to_string(),
                description: "Tailscale admin API provider for omnifs".to_string(),
            },
        ))
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["api.tailscale.com".to_string()],
            auth_types: vec![],
            max_memory_mb: 32,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 0,
        }
    }
}
```

Notes:

- `on_event` is deliberately omitted. The worktree's old provider had no
  event handler either; there is no `CacheInvalidate*` effect to port.
  If in the future we add a periodic refresh of devices/users/policy,
  re-introduce `on_event` matching the github provider's shape (see
  "Event handling migration" below for a concrete skeleton).
- `refresh_interval_secs: 0` disables the timer tick entirely, matching
  the pre-migration behavior.

### `providers/tailscale/src/http_ext.rs` — keep, minor refresh

Old file already uses `Cx<State>` + `omnifs_sdk::http::Request`. Only
change: the state field accessed is still `auth_header` (unchanged),
and both helpers continue to set the `Authorization` header from it.
No functional change required; keep the file as-is.

For reference, the kept file is:

```rust
use omnifs_sdk::Cx;
use omnifs_sdk::http::Request;

use crate::State;

pub(crate) trait TailscaleHttpExt {
    fn tailscale_json_get(&self, url: impl Into<String>) -> Request<'_, State>;
    fn tailscale_hujson_get(&self, url: impl Into<String>) -> Request<'_, State>;
}

impl TailscaleHttpExt for Cx<State> {
    fn tailscale_json_get(&self, url: impl Into<String>) -> Request<'_, State> {
        let auth_header = self.state(|state| state.auth_header.clone());
        self.http()
            .get(url)
            .header("Authorization", auth_header)
            .header("Accept", "application/json")
    }

    fn tailscale_hujson_get(&self, url: impl Into<String>) -> Request<'_, State> {
        let auth_header = self.state(|state| state.auth_header.clone());
        self.http()
            .get(url)
            .header("Authorization", auth_header)
            .header("Accept", "application/hujson")
    }
}
```

Verification: no edits needed. The `Cx<State>`, `Request<'_, State>`,
and `self.state(..)` surface is identical on the new SDK. Callers inside
`api.rs` continue to invoke `self.cx.tailscale_json_get(url).send_body()`
and `self.cx.tailscale_hujson_get(url).send()`.

### `providers/tailscale/src/api.rs` — keep almost verbatim

The old file already sits on top of `Cx<State>`, `Request::send_body`,
and `Request::send`. These types exist unchanged on the new SDK. The
only adjustment is to replace the `crate::ProviderResult` alias with
`crate::Result` (they are the same type; the symbol moves). The
`parse_model`, `to_json_bytes`, `effect_result_to_body`, and
`header_value` helpers stay put. All `DeviceRecord` / `UserRecord` /
`PolicySnapshot` / `DnsSnapshot` fields are preserved.

Full replacement file (only the `ProviderResult` → `Result` rename):

```rust
use std::collections::BTreeMap;

use omnifs_sdk::{Cx, prelude::*};
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::Result;
use crate::State;
use crate::http_ext::TailscaleHttpExt;
use crate::types::{NodeId, UserId};

const API_BASE: &str = "https://api.tailscale.com";

pub(crate) struct ApiClient<'cx> {
    cx: &'cx Cx<State>,
}

#[derive(Clone, Debug)]
pub(crate) struct DeviceRecord {
    pub(crate) node_id: NodeId,
    pub(crate) name: String,
    pub(crate) hostname: String,
    pub(crate) user: String,
    pub(crate) tags_json: Vec<u8>,
    pub(crate) addresses_json: Vec<u8>,
    pub(crate) os: String,
    pub(crate) client_version: String,
    pub(crate) authorized: bool,
    pub(crate) connected_to_control: bool,
    pub(crate) created: String,
    pub(crate) last_seen: String,
    pub(crate) key_expiry_disabled: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct UserRecord {
    pub(crate) id: UserId,
    pub(crate) display_name: String,
    pub(crate) login_name: String,
    pub(crate) role: String,
    pub(crate) status: String,
    pub(crate) relation_type: String,
    pub(crate) device_count: u64,
    pub(crate) last_seen: String,
    pub(crate) currently_connected: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct PolicySnapshot {
    pub(crate) hujson: String,
    pub(crate) etag: String,
}

#[derive(Clone, Debug)]
pub(crate) struct DnsSnapshot {
    pub(crate) nameservers: Vec<u8>,
    pub(crate) searchpaths: Vec<u8>,
    pub(crate) preferences: Vec<u8>,
    pub(crate) split_dns: Vec<u8>,
}

impl<'cx> ApiClient<'cx> {
    pub(crate) fn new(cx: &'cx Cx<State>) -> Self {
        Self { cx }
    }

    pub(crate) async fn list_devices(&self) -> Result<Vec<DeviceRecord>> {
        let url = self.tailnet_url(&["devices"])?;
        let body = self.cx.tailscale_json_get(url).send_body().await?;
        let response: DevicesResponse = parse_model(&body)?;
        response
            .devices
            .into_iter()
            .map(DeviceRecord::try_from_wire)
            .collect()
    }

    pub(crate) async fn find_device(&self, node_id: &NodeId) -> Result<DeviceRecord> {
        self.list_devices()
            .await?
            .into_iter()
            .find(|device| &device.node_id == node_id)
            .ok_or_else(|| ProviderError::not_found("device not found"))
    }

    pub(crate) async fn list_users(&self) -> Result<Vec<UserRecord>> {
        let url = self.tailnet_url(&["users"])?;
        let body = self.cx.tailscale_json_get(url).send_body().await?;
        let response: UsersResponse = parse_model(&body)?;
        response
            .users
            .into_iter()
            .map(UserRecord::from_wire)
            .collect()
    }

    pub(crate) async fn find_user(&self, user_id: &UserId) -> Result<UserRecord> {
        self.list_users()
            .await?
            .into_iter()
            .find(|user| &user.id == user_id)
            .ok_or_else(|| ProviderError::not_found("user not found"))
    }

    pub(crate) async fn get_policy(&self) -> Result<PolicySnapshot> {
        let url = self.tailnet_url(&["acl"])?;
        let response = self.cx.tailscale_hujson_get(url).send().await?;
        let hujson = String::from_utf8(response.body).map_err(|error| {
            ProviderError::invalid_input(format!("policy is not UTF-8: {error}"))
        })?;
        Ok(PolicySnapshot {
            hujson,
            etag: header_value(&response.headers, "etag").unwrap_or_default(),
        })
    }

    pub(crate) async fn get_dns(&self) -> Result<DnsSnapshot> {
        let nameservers_url = self.tailnet_url(&["dns", "nameservers"])?;
        let searchpaths_url = self.tailnet_url(&["dns", "searchpaths"])?;
        let preferences_url = self.tailnet_url(&["dns", "preferences"])?;
        let split_dns_url = self.tailnet_url(&["dns", "split-dns"])?;

        let responses = join_all([
            self.cx.tailscale_json_get(nameservers_url).send_body(),
            self.cx.tailscale_json_get(searchpaths_url).send_body(),
            self.cx.tailscale_json_get(preferences_url).send_body(),
            self.cx.tailscale_json_get(split_dns_url).send_body(),
        ])
        .await;

        let mut responses = responses.into_iter();
        let Some(nameservers_body) = responses.next() else {
            return Err(ProviderError::internal(
                "DNS nameserver response was missing".to_string(),
            ));
        };
        let Some(searchpaths_body) = responses.next() else {
            return Err(ProviderError::internal(
                "DNS searchpaths response was missing".to_string(),
            ));
        };
        let Some(preferences_body) = responses.next() else {
            return Err(ProviderError::internal(
                "DNS preferences response was missing".to_string(),
            ));
        };
        let Some(split_dns_body) = responses.next() else {
            return Err(ProviderError::internal(
                "DNS split-dns response was missing".to_string(),
            ));
        };

        let nameservers = parse_model::<NameserversResponse>(&nameservers_body?)?;
        let searchpaths = parse_model::<SearchpathsResponse>(&searchpaths_body?)?;
        let preferences = parse_model::<DnsPreferencesWire>(&preferences_body?)?;
        let split_dns =
            parse_model::<BTreeMap<String, Vec<String>>>(&split_dns_body?)?;

        Ok(DnsSnapshot {
            nameservers: to_json_bytes(&nameservers.dns)?,
            searchpaths: to_json_bytes(&searchpaths.search_paths)?,
            preferences: to_json_bytes(&preferences)?,
            split_dns: to_json_bytes(&split_dns)?,
        })
    }

    fn tailnet_url(&self, segments: &[&str]) -> Result<String> {
        let tailnet = self.cx.state(|state| state.tailnet.clone());
        let mut url = Url::parse(API_BASE)
            .map_err(|error| ProviderError::internal(format!("invalid API base URL: {error}")))?;
        {
            let mut path_segments = url.path_segments_mut().map_err(|()| {
                ProviderError::internal("API base URL cannot accept path segments".to_string())
            })?;
            path_segments.extend(["api", "v2", "tailnet"]);
            path_segments.push(&tailnet);
            path_segments.extend(segments.iter().copied());
        }
        Ok(url.to_string())
    }
}

#[derive(Debug, Deserialize)]
struct DevicesResponse {
    devices: Vec<DeviceWire>,
}

#[derive(Debug, Deserialize)]
struct DeviceWire {
    #[serde(default)]
    addresses: Vec<String>,
    #[serde(default)]
    name: String,
    #[serde(rename = "nodeId")]
    node_id: NodeId,
    #[serde(default)]
    authorized: bool,
    #[serde(default)]
    user: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(rename = "keyExpiryDisabled", default)]
    key_expiry_disabled: bool,
    #[serde(rename = "clientVersion", default)]
    client_version: String,
    #[serde(default)]
    created: String,
    #[serde(default)]
    hostname: String,
    #[serde(rename = "connectedToControl", default)]
    connected_to_control: bool,
    #[serde(rename = "lastSeen", default)]
    last_seen: Option<String>,
    #[serde(default)]
    os: String,
}

impl DeviceRecord {
    fn try_from_wire(wire: DeviceWire) -> Result<Self> {
        Ok(Self {
            node_id: wire.node_id,
            name: wire.name,
            hostname: wire.hostname,
            user: wire.user,
            tags_json: to_json_bytes(&wire.tags)?,
            addresses_json: to_json_bytes(&wire.addresses)?,
            os: wire.os,
            client_version: wire.client_version,
            authorized: wire.authorized,
            connected_to_control: wire.connected_to_control,
            created: wire.created,
            last_seen: wire.last_seen.unwrap_or_default(),
            key_expiry_disabled: wire.key_expiry_disabled,
        })
    }
}

#[derive(Debug, Deserialize)]
struct UsersResponse {
    users: Vec<UserWire>,
}

#[derive(Debug, Deserialize)]
struct UserWire {
    id: UserId,
    #[serde(rename = "displayName", default)]
    display_name: String,
    #[serde(rename = "loginName", default)]
    login_name: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    status: String,
    #[serde(rename = "type", default)]
    relation_type: String,
    #[serde(rename = "deviceCount", default)]
    device_count: u64,
    #[serde(rename = "lastSeen", default)]
    last_seen: String,
    #[serde(rename = "currentlyConnected", default)]
    currently_connected: bool,
}

impl UserRecord {
    fn from_wire(wire: UserWire) -> Result<Self> {
        Ok(Self {
            id: wire.id,
            display_name: wire.display_name,
            login_name: wire.login_name,
            role: wire.role,
            status: wire.status,
            relation_type: wire.relation_type,
            device_count: wire.device_count,
            last_seen: wire.last_seen,
            currently_connected: wire.currently_connected,
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct NameserversResponse {
    dns: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct SearchpathsResponse {
    #[serde(rename = "searchPaths")]
    search_paths: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DnsPreferencesWire {
    #[serde(rename = "magicDNS")]
    magic_dns: bool,
}

fn parse_model<T>(body: &[u8]) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_slice(body)
        .map_err(|error| ProviderError::invalid_input(format!("JSON parse error: {error}")))
}

fn to_json_bytes<T>(value: &T) -> Result<Vec<u8>>
where
    T: Serialize,
{
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        ProviderError::internal(format!("failed to encode projected JSON content: {error}"))
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn header_value(
    headers: &[omnifs_sdk::omnifs::provider::types::Header],
    name: &str,
) -> Option<String> {
    headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value.clone())
}

#[cfg(test)]
mod tests {
    use super::to_json_bytes;

    #[test]
    fn json_projection_bytes_end_with_newline() {
        let bytes = to_json_bytes(&vec!["one", "two"]).expect("projection JSON");
        assert!(bytes.ends_with(b"\n"));
    }
}
```

Notes on the api.rs diff:

- `ProviderResult` → `Result`. Every occurrence is updated.
- The old `effect_result_to_body(result: Result<Vec<u8>, ProviderResponse>)`
  helper is deleted. On the new SDK `send_body()` futures resolve to
  `Result<Vec<u8>, ProviderError>` directly (same shape as every other
  `Result<T>` in the provider), so `.await?` propagates the error
  without a converter. In the refactored `get_dns`, each
  `*_body` is a `Result<Vec<u8>>` and we `?` it before feeding into
  `parse_model`.
- `Header` was previously imported via the old SDK prelude. It lives in
  `omnifs_sdk::omnifs::provider::types::Header` on the new SDK; the
  fully qualified path is used in `header_value` to avoid changing the
  prelude surface.
- `join_all` is re-exported by `omnifs_sdk::prelude::*`; no extra import.
- The test module still compiles on `wasm32-wasip2` via `cargo test
  -p omnifs-provider-tailscale --target wasm32-wasip2 --no-run`.

### `providers/tailscale/src/nodes.rs` — delete

Every `Dir` trait impl in `nodes.rs` folds into a free function in
`root.rs`. The file must be removed.

### `providers/tailscale/src/root.rs` — create

This is the new home of every path handler, exposing the single
`RootHandlers` struct referenced from `provider.rs`. All handlers are
free functions; the macros generate the dispatch table.

New file (full replacement):

```rust
use omnifs_sdk::prelude::*;

use crate::Result;
use crate::State;
use crate::api::{ApiClient, DeviceRecord, DnsSnapshot, PolicySnapshot, UserRecord};
use crate::types::{NodeId, UserId};

pub struct RootHandlers;

#[handlers]
impl RootHandlers {
    #[dir("/")]
    fn root(_cx: &DirCx<'_, State>) -> Result<Projection> {
        let mut projection = Projection::new();
        projection.dir("_devices");
        projection.dir("_users");
        projection.dir("_policy");
        projection.dir("_dns");
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[dir("/_devices")]
    fn devices(_cx: &DirCx<'_, State>) -> Result<Projection> {
        let mut projection = Projection::new();
        projection.dir("by-node");
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[dir("/_devices/by-node")]
    async fn devices_by_node(cx: &DirCx<'_, State>) -> Result<Projection> {
        let devices = ApiClient::new(cx).list_devices().await?;

        let mut projection = Projection::new();
        let mut preloads: Vec<(String, Vec<u8>)> = Vec::new();
        for device in &devices {
            let id = device.node_id.as_ref();
            projection.dir(id);

            let base = format!("/_devices/by-node/{id}");
            for (name, bytes) in device_files(device) {
                preloads.push((format!("{base}/{name}"), bytes));
            }
        }
        projection.preload_many(preloads);
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[dir("/_devices/by-node/{node_id}")]
    async fn device(cx: &DirCx<'_, State>, node_id: NodeId) -> Result<Projection> {
        let device = ApiClient::new(cx).find_device(&node_id).await?;
        let mut projection = Projection::new();
        for (name, bytes) in device_files(&device) {
            projection.file_with_content(name, bytes);
        }
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[dir("/_users")]
    fn users(_cx: &DirCx<'_, State>) -> Result<Projection> {
        let mut projection = Projection::new();
        projection.dir("by-id");
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[dir("/_users/by-id")]
    async fn users_by_id(cx: &DirCx<'_, State>) -> Result<Projection> {
        let users = ApiClient::new(cx).list_users().await?;

        let mut projection = Projection::new();
        let mut preloads: Vec<(String, Vec<u8>)> = Vec::new();
        for user in &users {
            let id = user.id.as_ref();
            projection.dir(id);

            let base = format!("/_users/by-id/{id}");
            for (name, bytes) in user_files(user) {
                preloads.push((format!("{base}/{name}"), bytes));
            }
        }
        projection.preload_many(preloads);
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[dir("/_users/by-id/{user_id}")]
    async fn user(cx: &DirCx<'_, State>, user_id: UserId) -> Result<Projection> {
        let user = ApiClient::new(cx).find_user(&user_id).await?;
        let mut projection = Projection::new();
        for (name, bytes) in user_files(&user) {
            projection.file_with_content(name, bytes);
        }
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[dir("/_policy")]
    async fn policy(cx: &DirCx<'_, State>) -> Result<Projection> {
        let policy = ApiClient::new(cx).get_policy().await?;
        let mut projection = Projection::new();
        projection.file_with_content("acl.hujson", policy.hujson.into_bytes());
        projection.file_with_content("etag", policy.etag.into_bytes());
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[dir("/_dns")]
    async fn dns(cx: &DirCx<'_, State>) -> Result<Projection> {
        let dns = ApiClient::new(cx).get_dns().await?;
        let mut projection = Projection::new();
        for (name, bytes) in dns_files(&dns) {
            projection.file_with_content(name, bytes);
        }
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }
}

fn device_files(device: &DeviceRecord) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("name", device.name.clone().into_bytes()),
        ("hostname", device.hostname.clone().into_bytes()),
        ("user", device.user.clone().into_bytes()),
        ("tags.json", device.tags_json.clone()),
        ("addresses.json", device.addresses_json.clone()),
        ("os", device.os.clone().into_bytes()),
        ("client-version", device.client_version.clone().into_bytes()),
        ("authorized", device.authorized.to_string().into_bytes()),
        (
            "connected-to-control",
            device.connected_to_control.to_string().into_bytes(),
        ),
        ("created", device.created.clone().into_bytes()),
        ("last-seen", device.last_seen.clone().into_bytes()),
        (
            "key-expiry-disabled",
            device.key_expiry_disabled.to_string().into_bytes(),
        ),
    ]
}

fn user_files(user: &UserRecord) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("display-name", user.display_name.clone().into_bytes()),
        ("login-name", user.login_name.clone().into_bytes()),
        ("role", user.role.clone().into_bytes()),
        ("status", user.status.clone().into_bytes()),
        ("type", user.relation_type.clone().into_bytes()),
        ("device-count", user.device_count.to_string().into_bytes()),
        ("last-seen", user.last_seen.clone().into_bytes()),
        (
            "currently-connected",
            user.currently_connected.to_string().into_bytes(),
        ),
    ]
}

fn dns_files(dns: &DnsSnapshot) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("nameservers.json", dns.nameservers.clone()),
        ("searchpaths.json", dns.searchpaths.clone()),
        ("preferences.json", dns.preferences.clone()),
        ("split-dns.json", dns.split_dns.clone()),
    ]
}
```

Notes on the handler migration:

- The old `Dir` trait separated `load` (fetch data, needs `Cx`) from
  `project` (pure, produces children). The new SDK collapses both into
  one function: fetch inside the handler, build a `Projection`, return.
- The old `DevicesByNode::project` called `p.preload(path.device(node),
  clone)` to warm per-device entries. The new SDK's `Projection::preload`
  takes `(path, bytes)` pairs, not entity clones. We preload by
  synthesizing the absolute path (`/_devices/by-node/{id}/{file}`) for
  each sibling file of each listed device. This preserves the "listing
  warms sibling file cache" idiom; subsequent `read_file` against
  `/_devices/by-node/X/hostname` hits the host cache without a round
  trip.
- `Device::project` and `User::project` become `device`/`user` handlers
  that use `Projection::file_with_content` for eager bytes. Because the
  content length is always known, the host stores correct sizes
  (`file_with_content` auto-sizes from `bytes.len()`), satisfying the
  non-zero projected file size gotcha.
- `/_policy` and `/_dns` were previously dirs with inline files. They
  stay as dirs (not file handlers) because the underlying API returns
  multi-file data, preserving the shape observers expect.
- Typed captures (`NodeId`, `UserId`) are inferred from parameter
  types; `types.rs` keeps the `FromStr` impls and segment validation.

### `providers/tailscale/src/types.rs` — keep verbatim

No changes. `NodeId` and `UserId` continue to implement `FromStr` (the
trait bound the path macros require for typed captures), `AsRef<str>`,
`Display`, and the `is_safe_segment` guard. The inline tests still
compile under `--target wasm32-wasip2 --no-run`.

For reference, the kept file is:

```rust
use core::str::FromStr;

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct NodeId(String);

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct UserId(String);

impl FromStr for NodeId {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        is_safe_segment(value)
            .then_some(Self(value.to_string()))
            .ok_or(())
    }
}

impl FromStr for UserId {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        is_safe_segment(value)
            .then_some(Self(value.to_string()))
            .ok_or(())
    }
}

impl AsRef<str> for NodeId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for UserId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for NodeId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl core::fmt::Display for UserId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

fn is_safe_segment(value: &str) -> bool {
    if value.is_empty() || value.starts_with('.') {
        return false;
    }

    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

#[cfg(test)]
mod tests {
    use super::{NodeId, UserId};

    #[test]
    fn ids_reject_empty_hidden_and_slash_segments() {
        assert!("node-1".parse::<NodeId>().is_ok());
        assert!("user-1".parse::<UserId>().is_ok());
        assert!("".parse::<NodeId>().is_err());
        assert!(".hidden".parse::<NodeId>().is_err());
        assert!("bad/name".parse::<UserId>().is_err());
        assert!("bad name".parse::<UserId>().is_err());
    }
}
```

## Event handling migration (`CacheInvalidate*` → `EventOutcome`)

The pre-migration tailscale provider emitted no `CacheInvalidate*`
effects; the host owned all caching and relied on capacity eviction.
There is therefore no existing event handling to port.

If a future version wants to refresh device/user/policy periodically,
wire an `on_event` handler on the provider impl in the same shape as
github's `events.rs`:

```rust
async fn on_event(_cx: Cx<State>, event: ProviderEvent) -> Result<EventOutcome> {
    let mut outcome = EventOutcome::new();
    match event {
        ProviderEvent::TimerTick(_) => {
            // Example: blow away the dynamic subtrees so the next FUSE
            // access re-fetches from the admin API.
            outcome.invalidate_prefix("/_devices/by-node");
            outcome.invalidate_prefix("/_users/by-id");
            outcome.invalidate_path("/_policy");
            outcome.invalidate_path("/_dns");
        },
        _ => {},
    }
    Ok(outcome)
}
```

That also requires setting `refresh_interval_secs` in
`RequestedCapabilities` to a non-zero value. For now keep
`refresh_interval_secs: 0` and omit `on_event` — the host's capacity-
bounded cache and manual FUSE invalidation are sufficient.

## Cargo.toml changes

### `providers/tailscale/Cargo.toml`

No dependency changes. `base64`, `omnifs-sdk`, `serde`, `serde_json`,
and `url` are all still used. The `[package.metadata.component]`
section stays (vestigial, kept for documentation of the WIT world
mapping, same as the other providers). The `[lints.*]` blocks match
the dns and github providers and need no change.

### Worktree root `Cargo.toml`

After `git checkout --theirs -- Cargo.toml` during the merge, the file
will look exactly like main's (no `providers/tailscale`). Re-add
tailscale to `members`:

```toml
[workspace]
resolver = "2"
members = [
    "crates/*",
    "providers/github",
    "providers/dns",
    "providers/tailscale",
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

## Verification

Run from the worktree root after applying every edit above. Each
command must succeed before moving to the next.

```bash
cd /Users/raul/W/gvfs/.worktrees/providers/tailscale

# Formatting gate.
cargo fmt --check

# Provider lints for the wasm target (worker side).
cargo clippy -p omnifs-provider-tailscale --target wasm32-wasip2 -- -D warnings

# Provider test compile (cannot execute under wasm32-wasip2).
cargo test -p omnifs-provider-tailscale --target wasm32-wasip2 --no-run

# Full provider check (fmt + clippy + test-compile for every provider).
just check-providers
```

`just check-providers` is the authoritative gate; the three preceding
commands exist so a failure is diagnosed earlier. Do not skip any of
them.

## Risks and gotchas

- **API key auth shape.** Tailscale admin API keys authenticate as HTTP
  Basic with the key as the username and an empty password. That has
  not changed; `auth_header` is still `Basic base64(api_key:)`. Do not
  swap to `Authorization: Bearer` — it will yield 401. `auth_types` in
  `RequestedCapabilities` stays empty (no host-side auth negotiation);
  the provider builds its own header from the `api_key` config field.
  If a future version accepts OAuth client credentials, extend `Config`
  rather than reusing `api_key`.
- **Tailnet selection.** The config field `tailnet` is required and
  validated non-empty in `init`. For single-tailnet API keys the
  special value `-` selects the caller's default tailnet; the provider
  does not special-case it and will pass through whatever the config
  says. Document this in mount config instructions, not in code.
- **Device online state.** `connected_to_control` and `last_seen` come
  from a single `list_devices` call, so they reflect the moment of the
  last listing only. With no `on_event` handler, the host cache can
  serve stale online state until the cache entry is evicted. This
  matches the pre-migration behavior and is intentional for this pass.
  If stale online state becomes a problem, migrate to the `TimerTick`
  pattern in "Event handling migration" above and invalidate
  `/_devices/by-node` on every tick.
- **Policy HuJSON formatting.** `get_policy` decodes the response as
  UTF-8 and treats it as an opaque blob. Tailscale's ACL endpoint can
  return canonicalized JSON vs. HuJSON depending on `Accept` header;
  the provider sends `Accept: application/hujson`, which preserves
  comments and trailing commas. Do not canonicalize before writing to
  the `acl.hujson` projected file — downstream edits rely on the
  original formatting round-tripping.
- **DNS preferences shape.** The `DnsPreferencesWire` struct captures
  only `magicDNS`. The Tailscale API adds fields over time; unknown
  fields are silently dropped by `serde(deny_unknown_fields)`? No — we
  do not set `deny_unknown_fields`, so the struct quietly ignores new
  fields. The projected `preferences.json` therefore reflects only
  known fields, not the raw API response. If a future requirement is to
  surface everything, replace `DnsPreferencesWire` with `serde_json::
  Value` and serialize via `to_json_bytes`.
- **`join_all` order invariant.** `get_dns` fans out four HTTP callouts
  via `join_all` and expects responses in declaration order. The SDK
  comments guarantee this ordering; do not reorder the four builders
  without also reordering the `responses.next()` consumers.
- **Projected file size ceilings.** `Projection::file_with_content`
  refuses bodies larger than 64 KiB. Tailscale ACLs and DNS split-DNS
  blobs can occasionally exceed that. If a real tailnet trips the
  ceiling, fall back to `file_with_stat` + a dedicated `#[file]`
  handler that serves the large payload on demand (same pattern dns
  uses for record files).
- **Preload paths must be absolute.** In `devices_by_node` and
  `users_by_id` the preloaded paths start with `/_devices/by-node/..`
  and `/_users/by-id/..` respectively; they must not be relative. The
  host keys its cache by absolute path.
- **Worktree merge must not keep old `crates/`.** Accidentally taking
  `--ours` on any file under `crates/` or `wit/` during the merge will
  leave the worktree pinned to the pre-redesign SDK and every handler
  in this plan will fail to compile. The `git checkout --theirs --
  crates wit` step is mandatory.

---

## Verification

- `cargo fmt --check`
- `cargo clippy -p omnifs-provider-tailscale --target wasm32-wasip2 -- -D warnings`
- `cargo test -p omnifs-provider-tailscale --target wasm32-wasip2 --no-run`
- `just check-providers`

All must pass. If `just` is not on PATH, note that in the PR body and run
the equivalent `cargo` commands from the root of this branch's worktree.

## Commit

Conventional:

```
feat(tailscale): migrate provider to path-first handler SDK
```

Body: one paragraph naming the major structural changes and the base SDK
PRs (#28 `feat/sdk-http-post-support`, #29 `feat/sdk-path-rest-captures`,
#D `feat/sdk-error-constructors`).

## PR

- Title: `feat(tailscale): migrate provider to path-first handler SDK`
- Body: summary + link to this branch's `PLAN.md` + verification results
  (which cargo commands ran, which passed, which were skipped and why).
