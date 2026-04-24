# feat/migrate-npm

The npm provider at `providers/npm/` in this worktree was written against the **old** omnifs SDK (`mounts!` macro, `Dir`/`File` traits, `Projection<'_, Path>`, `materialize()`, scope/identity keys).

## Blocked by

This plan cannot start execution until both of these have merged into `main`:

- PR #28 `feat/sdk-http-post-support` — https://github.com/raulk/omnifs/pull/28
- PR #29 `feat/sdk-path-rest-captures` — https://github.com/raulk/omnifs/pull/29

Note: `ProviderError::rate_limited` / `::permission_denied` / `::version_mismatch`
constructors are already on `main` (landed with the #27 refactor). No separate PR
is needed.

## Execution model

This branch was created off `main` at `6343486`. To execute:

1. `git -C /Users/raul/W/gvfs worktree add /Users/raul/W/gvfs/.worktrees/migrate-npm feat/migrate-npm`
2. Work in that worktree only.
3. Bring in the provider source from the old worktree at
   `/Users/raul/W/gvfs/.worktrees/providers/npm/providers/npm/`
   per the "Port provider source" step below.
4. Execute this PLAN.md end-to-end. Corrections in the "Migration
   corrections" section are authoritative over anything in the reference
   body that contradicts them.
5. Run the Verification commands listed near the bottom.
6. Commit on the `feat/migrate-npm` branch, push, open PR.


## Port provider source

This branch is off `main` at `6343486`, so there is NO merge from
`wip/provider-npm-impl` and NO `git merge main`. The wip branch carries OLD-SDK infrastructure
that must not land here. Only provider-local files come over, file by file,
using `git checkout <old-branch> -- <path>` (this pulls the file contents into
the working tree and index without touching anything else).

### Files to copy verbatim (no touch-ups beyond rust import paths / `ProviderResult` → `Result`)

- `providers/npm/src/types.rs`
- `providers/npm/src/package.rs`

Bring each over with:

```bash
git checkout wip/provider-npm-impl -- providers/npm/src/types.rs
git checkout wip/provider-npm-impl -- providers/npm/src/package.rs
```

### Files to copy then touch up

- `providers/npm/src/api.rs` — copy over, then apply the corrections in
  the relevant reference-body sections (auth removal, POST rewrites, error
  constructor substitutions).

Bring them in with:

```bash
git checkout wip/provider-npm-impl -- providers/npm/src/api.rs
```

Then edit in place.

### Files to create fresh (do NOT copy from the wip branch)

- `providers/npm/src/lib.rs`
- `providers/npm/src/provider.rs`
- `providers/npm/src/handlers/ (packages, users, orgs)`

### Files to keep only if structurally unchanged

- `providers/npm/src/root.rs` — if the new handler layout keeps the same
  `root.rs`-style module, copy over; otherwise rewrite.

### Files to DISCARD (do NOT bring to this branch)

- `providers/npm/src/old root.rs (if structure changes)`
- `providers/npm/src/old provider.rs`
- `providers/npm/src/old lib.rs`

These are old-SDK artifacts (entity projections, tree walkers, routes tables,
manual http_ext wrappers for auth). The new SDK shape replaces them with
path-first handlers.

### Bring over the provider Cargo.toml

```bash
git checkout wip/provider-npm-impl -- providers/npm/Cargo.toml
```

Then update its SDK dependency declarations to match `providers/github/Cargo.toml`
on the current `main`. In particular, `omnifs-sdk` must point at the workspace
version and not an old path/git revision.

### Re-register the provider in the workspace

The workspace-level `Cargo.toml` on `main` dropped every non-dns/github/test
provider. Re-add `providers/npm` to its `members` array. Example diff:

```toml
[workspace]
members = [
    "crates/cli",
    "crates/host",
    "providers/dns",
    "providers/github",
+   "providers/npm",
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
    domains: vec!["registry.npmjs.org".to_string(), "www.npmjs.com".to_string()],
    ..Default::default()
}
```

If an operator wants to raise rate limits by configuring a token on the
mount, the host can inject it transparently; the provider code does not
need to know. Do NOT add `token`, `api_key`, or similar fields to
`Config` or `State`. Do NOT add manual `Authorization` headers.

Domains covered:

  - `registry.npmjs.org`
  - `www.npmjs.com`

Mount config shape (anonymous is default; optional auth shown):

```json
{
  "plugin": "npm.wasm",
  "mount": "/npm",
  "auth": [{"type": "bearer-token", "token_env": "NPM_TOKEN", "domain": "registry.npmjs.org"}]
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
> `/Users/raul/W/gvfs/.worktrees/providers/npm/MIGRATION_PLAN.md`.
> Read it for provider-specific shape, path tables, gotchas, and per-file
> migration notes. Wherever a passage conflicts with the corrections above
> (auth handling, POST shape, error constructors, rest captures, destructive
> action for crates-io), the corrections win.

# NPM provider migration plan

## Summary

The npm provider at `providers/npm/` in this worktree was written against
the **old** omnifs SDK (`mounts!` macro, `Dir`/`File` traits,
`Projection<'_, Path>`, `materialize()`, scope/identity keys). The SDK
on `main` (commit `6343486`) replaces this surface with free-function
path handlers declared via `#[dir("...")]` / `#[file("...")]` /
`#[subtree("...")]` inside `#[handlers] impl ...` blocks, registered
through `#[provider(mounts(...))]`. `Projection` is now owned, callouts
replace effects, caching is host-owned without TTLs, and cache signals
travel on terminals (`preload`) or `EventOutcome` instead of separate
effects.

This plan:

1. Pulls main's SDK/WIT into the worktree via `git merge main` (taking
   main for `crates/` and `wit/`).
2. Rewrites every file under `providers/npm/src/` against the new SDK
   while preserving the npm REST client (`api.rs`) and domain types
   (`types.rs`).
3. Handles scoped packages (`@scope/name`) by declaring **two handler
   sets** per resource (scoped + unscoped), because the path-pattern
   engine does **not** support rest captures and `/` is always a
   segment separator. This matches the strategy already encoded in the
   old `mounts!` table.
4. Registers the provider in the workspace `Cargo.toml` and verifies
   with the standard provider checks.

Worktree tip: `e1d0b85`. Fork point with `main`: `7742e99`. Commit count
ahead: 5.

## Current path table (verbatim from the old `mounts!`)

From `providers/npm/src/lib.rs` (old):

```rust
omnifs_sdk::mounts! {
    capture scope: crate::types::ScopeName;
    capture package: crate::types::PackageName;
    capture query: String;
    capture version: String;

    "/" (dir) => Root;
    "/_keys.json" (file) => RegistryKeys;
    "/_search" (dir) => SearchRoot;
    "/_search/{query}" (dir) => SearchResults;
    "/@{scope}" (dir) => ScopeRoot;
    "/@{scope}/{package}" (dir as Scoped) => Package;
    "/@{scope}/{package}/README.md" (file as Scoped) => Readme;
    "/@{scope}/{package}/dist-tags" (dir as Scoped) => DistTags;
    "/@{scope}/{package}/versions" (dir as Scoped) => Versions;
    "/@{scope}/{package}/versions/{version}" (dir as Scoped) => Version;
    "/{package}" (dir as Unscoped) => Package;
    "/{package}/README.md" (file as Unscoped) => Readme;
    "/{package}/dist-tags" (dir as Unscoped) => DistTags;
    "/{package}/versions" (dir as Unscoped) => Versions;
    "/{package}/versions/{version}" (dir as Unscoped) => Version;
}
```

Projected child files per entity (derived from `src/package.rs` and
`src/root.rs`):

| Dir handler | Projected file children (name : content source) |
|---|---|
| `Package` | `name`, `modified?`, `latest?` |
| `DistTags` | `<tag>` → `<version>` per dist-tag |
| `Versions` | `<version>` subdirs (one per published version) |
| `Version` | `version`, `deprecated?`, `package.json`, `dependencies.json`, `integrity?`, `shasum?`, `unpacked-size?`, `file-count?`, `tarball-url`, `signatures.json` |
| `SearchResults` | `results.json` |
| `Root`, `ScopeRoot`, `SearchRoot` | *(empty; presence only)* |

File handlers (leaf file routes):

| File handler | Source |
|---|---|
| `RegistryKeys` (`/_keys.json`) | `GET {base}/-/npm/v1/keys` |
| `Readme` (`…/README.md`) | `readme` field from the full packument |

## Target path table (new SDK)

Same URL shape, split into scoped/unscoped variants to match the
path-pattern engine (see "Scoped-package capture" below). Template
names use new SDK conventions (owned `Projection`, typed captures).

| Template | Kind | Handler fn (in `RootHandlers` / `PackageHandlers`) |
|---|---|---|
| `/` | dir | `RootHandlers::root` |
| `/_keys.json` | file | `RootHandlers::registry_keys` |
| `/_search` | dir | `RootHandlers::search_root` |
| `/_search/{query}` | dir | `RootHandlers::search_results` |
| `/@{scope}` | dir | `RootHandlers::scope_root` |
| `/@{scope}/{package}` | dir | `PackageHandlers::scoped_package` |
| `/@{scope}/{package}/README.md` | file | `PackageHandlers::scoped_readme` |
| `/@{scope}/{package}/dist-tags` | dir | `PackageHandlers::scoped_dist_tags` |
| `/@{scope}/{package}/versions` | dir | `PackageHandlers::scoped_versions` |
| `/@{scope}/{package}/versions/{version}` | dir | `PackageHandlers::scoped_version` |
| `/{package}` | dir | `PackageHandlers::unscoped_package` |
| `/{package}/README.md` | file | `PackageHandlers::unscoped_readme` |
| `/{package}/dist-tags` | dir | `PackageHandlers::unscoped_dist_tags` |
| `/{package}/versions` | dir | `PackageHandlers::unscoped_versions` |
| `/{package}/versions/{version}` | dir | `PackageHandlers::unscoped_version` |

Each scoped/unscoped pair delegates to a shared async helper (e.g.
`package_projection(cx, scope_opt, package).await`) so the npm HTTP
logic is written once.

## Scoped-package capture: verified

`crates/omnifs-mount-schema/src/lib.rs` (main) implements `PathPattern`
with two segment kinds: `Literal(String)` and `Capture { name, prefix:
Option<String> }`. Parsing of a template segment:

- A bare `{name}` becomes `Capture { name, prefix: None }` and matches
  exactly **one non-empty path segment**.
- A `prefix{name}` becomes `Capture { name, prefix: Some(prefix) }` and
  matches one segment whose content starts with `prefix` and has
  non-empty remainder (e.g. `@{scope}` matches `@foo` and captures
  `foo`; it does **not** match `@foo/bar`).
- `{*rest}`/`{*...}` tokens **error out explicitly**:
  ```rust
  if raw.starts_with("{*") {
      return Err(pattern_error(format!(
          "rest captures are not supported in {raw:?}"
      )));
  }
  ```

Consequence: a hypothetical `/_packages/{pkg}` pattern with `pkg =
"@scope/name"` would never match, because the `/` in `@scope/name`
forces two path segments but the template only reserves one. The old
`mounts!` table already worked around this by declaring scoped and
unscoped routes separately (`"/@{scope}/{package}"` vs
`"/{package}"`). The migration keeps that strategy.

Do **not** invent `/_packages/{pkg}` or any rest-capture template; the
SDK will fail `MountRegistry::add_dir` at init time with an
`invalid-input` error. Use the two-handler pattern in every case.

## SDK cheatsheet (verbatim, inline)

All of this is available via `use omnifs_sdk::prelude::*;`:

**Errors and result type**

```rust
pub type Result<T> = core::result::Result<T, ProviderError>;

// Constructors (all accept impl Into<String>):
ProviderError::not_found(msg);
ProviderError::not_a_directory(msg);
ProviderError::not_a_file(msg);
ProviderError::permission_denied(msg);
ProviderError::invalid_input(msg);
ProviderError::network(msg);
ProviderError::timeout(msg);
ProviderError::denied(msg);
ProviderError::too_large(msg);
ProviderError::rate_limited(msg);
ProviderError::version_mismatch(msg);
ProviderError::unimplemented(msg);
ProviderError::internal(msg);
ProviderError::from_http_status(status_u16); // maps HTTP code to kind
```

`ProviderErrorKind`: `NotFound`, `NotADirectory`, `NotAFile`,
`PermissionDenied`, `Network`, `Timeout`, `Denied`, `InvalidInput`,
`TooLarge`, `RateLimited`, `VersionMismatch`, `Unimplemented`,
`Internal`.

**Context** (`Cx<State>`)

```rust
cx.state(|s: &State| -> R { ... })
cx.state_mut(|s: &mut State| -> R { ... })
cx.http()           // HTTP builder; no git() needed for npm
cx.active_paths(mount_id, parse_fn) // on TimerTick events
```

**HTTP builder**

```rust
let req = cx.http().get(url)                        // starts a GET
    .header("Accept", "application/vnd.npm.install-v1+json")
    .header("Authorization", format!("Bearer {tok}"));
let bytes: Vec<u8> = req.send_body().await?;        // body only
// Or:
let resp: HttpResponse = cx.http().get(url).send().await?;
// resp.status, resp.headers (Vec<Header>), resp.body (Vec<u8>)
```

`send_body()` maps HTTP 4xx/5xx to a typed `ProviderError` via
`from_http_status` automatically. `send()` returns the raw response
(status + headers + body) so the caller can inspect e.g. `304 Not
Modified`; a non-success status is **not** mapped to an error with
`send()`, you get the `HttpResponse` back. Callout errors at the
transport layer map through `from_callout_error` either way.

There is no explicit POST/PUT builder in `crates/omnifs-sdk/src/http.rs`
today (only `get`). The npm provider only ever issues GETs, so this is
fine.

**Parallel callouts**

```rust
use omnifs_sdk::prelude::*; // re-exports join_all from cx.

let futures = urls.iter().map(|u| cx.http().get(u).send_body());
let bodies: Vec<Result<Vec<u8>>> = join_all(futures).await;
```

All child futures must be bound to the same `Cx`. Each child yields
exactly one callout per suspension.

**Projection (owned, `#[dir]`)**

```rust
let mut p = Projection::new();

p.dir("name");                                // child directory
p.file("name");                               // placeholder-sized file
p.file_with_stat("name", FileStat { size });  // explicit stat
p.file_with_content("name", bytes);           // eager content (<= 64 KiB)
p.page(PageStatus::Exhaustive);               // authoritative final page
p.page(PageStatus::More(Cursor::Opaque("next".into())));
p.preload("relative/or/absolute/path", bytes);
p.preload_many(iter_of_(path, bytes));
```

`MAX_PROJECTED_BYTES = 64 * 1024`. Content larger than 64 KiB is
silently rejected and converts the projection into an
`invalid_input` error at yield time (don't pre-check; the SDK does).

Duplicate entry names or `.` / `..` / `/`-bearing names are rejected
the same way. File names must be valid relative segments.

**FileContent (`#[file]`)**

```rust
FileContent::bytes(vec_or_slice)         // eager content
// Streaming/range variants exist but the current host runtime returns
// "unimplemented" for them, so stick to ::bytes() for npm.
```

Sibling files: there is no `FileContent::with_sibling_files(...)` in
the top-level `handler::FileContent` enum. Sibling-file preload for
file reads happens automatically when the file is served from an
ancestor `#[dir]` handler's `file_with_content(...)` projection. For a
direct `#[file]` handler, carry adjacency through `Projection::preload`
on the corresponding `#[dir]` handler's response (e.g. project sibling
files from the parent `Package` dir).

**`DirCx` and intents**

```rust
pub struct DirCx<'a, S> { /* private */ }
impl<S> core::ops::Deref for DirCx<'_, S> { type Target = Cx<S>; /* ... */ }

pub enum DirIntent<'a> {
    Lookup { child: &'a str },
    List   { cursor: Option<Cursor> },
    ReadProjectedFile { name: &'a str },
}

fn foo(cx: &DirCx<'_, State>) -> Result<Projection> {
    match cx.intent() { /* optional specialization */ }
    cx.http().get(...) // DirCx derefs to Cx<State>
    // ...
}
```

For npm you don't need to specialize on intent: the existing handlers
project fully every time; the host caches the result. If you want to
avoid re-fetching the packument on a `ReadProjectedFile`, match
`DirIntent::ReadProjectedFile { name }` and return early. That is a
performance nicety; the mechanical migration can ignore it.

**Typed captures**

Bare `String` works unchanged. Custom types must implement `FromStr`
with any error type (`Err = ()` is fine). The handler's parameter
order must match the left-to-right order of `{captures}` in the
template. `ScopeName` and `PackageName` in `types.rs` already
implement `FromStr`, unchanged.

**Subtree** — not used by npm. Ignore `#[subtree]`.

**Event outcomes** — not used by npm (no `on_event`); the provider
impl has no `on_event` function.

## Bring the worktree up to main

The worktree has forked crates/ and wit/ (the old `mounts!` macro and
old `Projection<'_, Path>` traits live in `crates/omnifs-sdk-macros/`,
`crates/omnifs-sdk/`, and friends). All of main's redesigned SDK and
WIT must replace those.

From the worktree root
(`/Users/raul/W/gvfs/.worktrees/providers/npm`):

```bash
git status                                  # confirm clean or expected dirty
git fetch origin main
git merge main
# For every conflict inside crates/ or wit/, take main's version:
git checkout --theirs -- crates wit
git add crates wit
# Any conflicts elsewhere (Cargo.toml, Cargo.lock, justfile) resolve
# manually, preferring main's structure (workspace member list,
# workspace deps). Then:
git commit
```

The existing worktree tree also contains `providers/npm/` which is
**untracked** (status shows `?? providers/npm/`). That directory
survives the merge untouched. After the merge, main's crates/ +
wit/ + providers/dns/ + providers/github/ + providers/test/ sit
alongside the untracked `providers/npm/` tree. This is the expected
state entering the source migration below.

Rebuild target/ hygiene (not required, optional):

```bash
cargo clean
```

## Cargo.toml changes

### Provider crate: `providers/npm/Cargo.toml`

The provider Cargo.toml is mostly fine. One change: keep the direct
`serde` + `serde_json` deps (the provider uses them explicitly in
`api.rs` and `types.rs`). Add `hashbrown` only if you introduce
hashbrown maps (the rewrite below does not). The `nodejs-semver` and
`percent-encoding` deps stay. Full replacement:

```toml
[package]
name = "omnifs-provider-npm"
version = "0.1.0"
edition = "2024"
description = "OmnIFS provider for browsing npm package metadata"
license = "MIT OR Apache-2.0"
repository = "https://github.com/raulk/omnifs"
homepage = "https://github.com/raulk/omnifs"

[lib]
crate-type = ["cdylib", "lib"]

[dependencies]
nodejs-semver = "4.2"
omnifs-sdk = { path = "../../crates/omnifs-sdk" }
percent-encoding = "2"
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

### Workspace Cargo.toml

Main's workspace `members` list does **not** include
`providers/npm`. After the merge, edit
`/Users/raul/W/gvfs/.worktrees/providers/npm/Cargo.toml` and add npm:

```toml
[workspace]
resolver = "2"
members = [
    "crates/*",
    "providers/github",
    "providers/dns",
    "providers/npm",
    "providers/test",
]
default-members = ["crates/cli", "crates/host"]
```

(Preserve the rest of main's workspace `Cargo.toml` verbatim: the
workspace-level `[workspace.dependencies]` and `[workspace.lints.*]`
sections from main are correct; do not regress them to the old
worktree's listing.)

## Per-file migration

All files live at
`/Users/raul/W/gvfs/.worktrees/providers/npm/providers/npm/src/`.

### `src/lib.rs` — REWRITE

Replaces `mounts!` with module declarations and a `Config` type using
`#[omnifs_sdk::config]`. Keeps `api.rs` and `types.rs`. Drops the
`ProviderResult<T>` alias (use SDK's `Result<T>` directly).

Full replacement:

```rust
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

pub(crate) use omnifs_sdk::prelude::Result;

mod api;
mod package;
mod provider;
mod root;
pub(crate) mod types;

#[derive(Clone)]
pub(crate) struct State {
    pub registry_base_url: String,
    pub search_page_size: u32,
}

#[omnifs_sdk::config]
pub struct Config {
    #[serde(default = "default_registry_base_url")]
    registry_base_url: String,
    #[serde(default = "default_preload_versions")]
    #[allow(dead_code)]
    preload_versions: usize,
    #[serde(default = "default_search_page_size")]
    search_page_size: u32,
}

fn default_registry_base_url() -> String {
    String::from("https://registry.npmjs.org")
}

fn default_preload_versions() -> usize {
    32
}

fn default_search_page_size() -> u32 {
    50
}
```

### `src/types.rs` — KEEP UNCHANGED

The file already compiles against the new SDK: the types don't
reference any removed SDK item. `ScopeName` / `PackageName` /
`canonical_package_name` / `sort_versions_desc` stay, and their
`FromStr` impls are what the new SDK expects for typed captures. The
tests at the bottom of the file are `#[cfg(test)]` pure-Rust and
compile under either target.

No edits.

### `src/api.rs` — LIGHTLY EDIT

The npm HTTP client stays. Only two imports change:

- Change `use crate::{ProviderResult, State};` to `use crate::State;`
  and use `Result` from `omnifs_sdk::prelude` (via `use
  omnifs_sdk::prelude::*;`).
- Change all `ProviderResult<T>` return types to `Result<T>`.
- Drop `omnifs_sdk::Cx` import if `prelude::*` pulls it in; actually
  `Cx` lives at `omnifs_sdk::Cx` (reexported at crate root; not in
  prelude). Keep the explicit `use omnifs_sdk::Cx;`.

Full replacement of `src/api.rs`:

```rust
use std::collections::BTreeMap;

use omnifs_sdk::Cx;
use omnifs_sdk::prelude::*;
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde::Deserialize;

use crate::State;
use crate::types::{PackageName, ScopeName, canonical_package_name};

const ABBREV_PACKUMENT_ACCEPT: &str = "application/vnd.npm.install-v1+json";
const PATH_COMPONENT_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');
const QUERY_COMPONENT_ENCODE_SET: &AsciiSet = &PATH_COMPONENT_ENCODE_SET.add(b'&').add(b'+').add(b'=');

pub(crate) struct NpmClient<'cx> {
    cx: &'cx Cx<State>,
    base_url: String,
    search_page_size: u32,
}

impl<'cx> NpmClient<'cx> {
    pub(crate) fn new(cx: &'cx Cx<State>) -> Self {
        let (base_url, search_page_size) =
            cx.state(|state| (state.registry_base_url.clone(), state.search_page_size));
        Self {
            cx,
            base_url,
            search_page_size,
        }
    }

    pub(crate) async fn packument_abbrev(
        &self,
        scope: Option<&ScopeName>,
        package: &PackageName,
    ) -> Result<AbbrevPackument> {
        self.get_json(&self.package_url(scope, package), Some(ABBREV_PACKUMENT_ACCEPT))
            .await
    }

    pub(crate) async fn packument_full(
        &self,
        scope: Option<&ScopeName>,
        package: &PackageName,
    ) -> Result<FullPackument> {
        self.get_json(&self.package_url(scope, package), None).await
    }

    pub(crate) async fn version(
        &self,
        scope: Option<&ScopeName>,
        package: &PackageName,
        version: &str,
    ) -> Result<VersionDoc> {
        let raw = self
            .get_value(&self.version_url(scope, package, version), None)
            .await?;
        VersionDoc::from_value(raw)
    }

    pub(crate) async fn search(&self, query: &str) -> Result<serde_json::Value> {
        self.get_value(&self.search_url(query), None).await
    }

    pub(crate) async fn keys(&self) -> Result<serde_json::Value> {
        self.get_value(&format!("{}/-/npm/v1/keys", self.base_url), None)
            .await
    }

    async fn get_json<T>(&self, url: &str, accept: Option<&str>) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let body = self.get_bytes(url, accept).await?;
        serde_json::from_slice(&body)
            .map_err(|error| ProviderError::invalid_input(format!("JSON parse error: {error}")))
    }

    async fn get_value(&self, url: &str, accept: Option<&str>) -> Result<serde_json::Value> {
        let body = self.get_bytes(url, accept).await?;
        serde_json::from_slice(&body)
            .map_err(|error| ProviderError::invalid_input(format!("JSON parse error: {error}")))
    }

    async fn get_bytes(&self, url: &str, accept: Option<&str>) -> Result<Vec<u8>> {
        let request = match accept {
            Some(accept) => self.cx.http().get(url).header("Accept", accept),
            None => self.cx.http().get(url),
        };
        request.send_body().await.map_err(Into::into)
    }

    fn package_url(&self, scope: Option<&ScopeName>, package: &PackageName) -> String {
        format!(
            "{}/{}",
            self.base_url,
            encode_path_component(&canonical_package_name(scope, package))
        )
    }

    fn version_url(
        &self,
        scope: Option<&ScopeName>,
        package: &PackageName,
        version: &str,
    ) -> String {
        format!(
            "{}/{}/{}",
            self.base_url,
            encode_path_component(&canonical_package_name(scope, package)),
            encode_path_component(version)
        )
    }

    fn search_url(&self, query: &str) -> String {
        format!(
            "{}/-/v1/search?text={}&size={}&from=0",
            self.base_url,
            encode_query_component(query),
            self.search_page_size
        )
    }
}

pub(crate) fn npm_client(cx: &Cx<State>) -> NpmClient<'_> {
    NpmClient::new(cx)
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct AbbrevPackument {
    pub name: String,
    #[serde(default)]
    pub modified: Option<String>,
    #[serde(rename = "dist-tags", default)]
    pub dist_tags: BTreeMap<String, String>,
    #[serde(default)]
    pub versions: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct FullPackument {
    #[serde(default)]
    pub readme: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct VersionDoc {
    pub version: String,
    pub deprecated: Option<String>,
    pub package_json: serde_json::Value,
    pub dependencies_json: serde_json::Value,
    pub integrity: Option<String>,
    pub shasum: Option<String>,
    pub unpacked_size: Option<u64>,
    pub file_count: Option<u64>,
    pub tarball_url: String,
    pub signatures_json: serde_json::Value,
}

impl VersionDoc {
    fn from_value(raw: serde_json::Value) -> Result<Self> {
        let parsed: VersionDocFields = serde_json::from_value(raw.clone()).map_err(|error| {
            ProviderError::invalid_input(format!("JSON parse error: {error}"))
        })?;

        Ok(Self {
            version: parsed.version,
            deprecated: parsed.deprecated,
            package_json: raw,
            dependencies_json: parsed
                .dependencies
                .map_or_else(|| serde_json::json!({}), serde_json::Value::Object),
            integrity: parsed.dist.integrity,
            shasum: parsed.dist.shasum,
            unpacked_size: parsed.dist.unpacked_size,
            file_count: parsed.dist.file_count,
            tarball_url: parsed.dist.tarball,
            signatures_json: serde_json::to_value(parsed.dist.signatures).map_err(|error| {
                ProviderError::invalid_input(format!("JSON serialize error: {error}"))
            })?,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
struct VersionDocFields {
    version: String,
    #[serde(default)]
    deprecated: Option<String>,
    #[serde(default)]
    dependencies: Option<serde_json::Map<String, serde_json::Value>>,
    dist: VersionDist,
}

#[derive(Clone, Debug, Deserialize)]
struct VersionDist {
    tarball: String,
    #[serde(default)]
    integrity: Option<String>,
    #[serde(default)]
    shasum: Option<String>,
    #[serde(rename = "unpackedSize", default)]
    unpacked_size: Option<u64>,
    #[serde(rename = "fileCount", default)]
    file_count: Option<u64>,
    #[serde(default)]
    signatures: Vec<RegistrySignature>,
}

#[derive(Clone, Debug, serde::Serialize, Deserialize)]
struct RegistrySignature {
    keyid: String,
    sig: String,
}

fn encode_path_component(value: &str) -> String {
    utf8_percent_encode(value, PATH_COMPONENT_ENCODE_SET).to_string()
}

fn encode_query_component(value: &str) -> String {
    utf8_percent_encode(value, QUERY_COMPONENT_ENCODE_SET).to_string()
}
```

### `src/provider.rs` — REWRITE

Replaces the old provider lifecycle with `#[provider(mounts(...))]`
wiring. No `on_event` (npm has no eventful flows today; stick to this
until a real trigger is added).

Full replacement:

```rust
use omnifs_sdk::prelude::*;

use crate::{Config, State};

#[provider(mounts(
    crate::root::RootHandlers,
    crate::package::PackageHandlers,
))]
impl NpmProvider {
    fn init(config: Config) -> Result<(State, ProviderInfo)> {
        let registry_base_url = config.registry_base_url.trim_end_matches('/').to_string();
        if registry_base_url.is_empty() {
            return Err(ProviderError::invalid_input(
                "registry_base_url must not be empty",
            ));
        }
        Ok((
            State {
                registry_base_url,
                search_page_size: config.search_page_size,
            },
            ProviderInfo {
                name: "npm-provider".to_string(),
                version: "0.1.0".to_string(),
                description: "npm package metadata browsing".to_string(),
            },
        ))
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["registry.npmjs.org".to_string()],
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

### `src/root.rs` — REWRITE

Old: four separate `impl Dir for ...` / `impl File for ...` blocks.
New: one `RootHandlers` struct with five free-function handlers. The
`Root`, `ScopeRoot`, `SearchRoot` marker structs are deleted: the host
derives their presence from sibling static templates. We use
`PageStatus::More(Cursor::Opaque("dynamic"))` for the browsable roots
(scope roots, search root, package root) that accept arbitrary
children, matching the DNS provider's approach.

Full replacement:

```rust
use omnifs_sdk::prelude::*;

use crate::api::npm_client;
use crate::types::ScopeName;
use crate::{Result, State};

const DYNAMIC_CURSOR: &str = "dynamic";

fn mark_dynamic(projection: &mut Projection) {
    projection.page(PageStatus::More(Cursor::Opaque(DYNAMIC_CURSOR.to_string())));
}

pub struct RootHandlers;

#[handlers]
impl RootHandlers {
    /// Registry root. Accepts `@{scope}` subdirs, `{package}` subdirs,
    /// `_keys.json`, and `_search`. Children are dynamic; static
    /// siblings (`_search`, `_keys.json`) are auto-injected from the
    /// matching `#[dir]`/`#[file]` templates, so we just mark the
    /// listing as non-exhaustive.
    #[dir("/")]
    fn root(_cx: &DirCx<'_, State>) -> Result<Projection> {
        let mut projection = Projection::new();
        mark_dynamic(&mut projection);
        Ok(projection)
    }

    #[file("/_keys.json")]
    async fn registry_keys(cx: &Cx<State>) -> Result<FileContent> {
        let keys = npm_client(cx).keys().await?;
        let bytes = serde_json::to_vec_pretty(&keys).map_err(|error| {
            ProviderError::invalid_input(format!("JSON serialize error: {error}"))
        })?;
        Ok(FileContent::bytes(bytes))
    }

    #[dir("/_search")]
    fn search_root(_cx: &DirCx<'_, State>) -> Result<Projection> {
        let mut projection = Projection::new();
        mark_dynamic(&mut projection);
        Ok(projection)
    }

    #[dir("/_search/{query}")]
    async fn search_results(
        cx: &DirCx<'_, State>,
        query: String,
    ) -> Result<Projection> {
        let results = npm_client(cx).search(&query).await?;
        let bytes = serde_json::to_vec_pretty(&results).map_err(|error| {
            ProviderError::invalid_input(format!("JSON serialize error: {error}"))
        })?;
        let mut projection = Projection::new();
        projection.file_with_content("results.json", bytes);
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[dir("/@{scope}")]
    fn scope_root(_cx: &DirCx<'_, State>, _scope: ScopeName) -> Result<Projection> {
        let mut projection = Projection::new();
        mark_dynamic(&mut projection);
        Ok(projection)
    }
}
```

### `src/package.rs` — REWRITE

This file holds the bulk of the migration: every `Dir`/`File` impl
becomes a pair of `#[handlers]` methods (scoped + unscoped). Each pair
delegates to a shared async helper so the npm REST logic is written
once.

Full replacement:

```rust
use omnifs_sdk::prelude::*;

use crate::api::{AbbrevPackument, VersionDoc, npm_client};
use crate::types::{PackageName, ScopeName, sort_versions_desc};
use crate::{Result, State};

pub struct PackageHandlers;

#[handlers]
impl PackageHandlers {
    // ---- Package directory: /@{scope}/{package} and /{package} ----

    #[dir("/@{scope}/{package}")]
    async fn scoped_package(
        cx: &DirCx<'_, State>,
        scope: ScopeName,
        package: PackageName,
    ) -> Result<Projection> {
        package_projection(cx, Some(&scope), &package).await
    }

    #[dir("/{package}")]
    async fn unscoped_package(
        cx: &DirCx<'_, State>,
        package: PackageName,
    ) -> Result<Projection> {
        package_projection(cx, None, &package).await
    }

    // ---- README file: /@{scope}/{package}/README.md and /{package}/README.md ----

    #[file("/@{scope}/{package}/README.md")]
    async fn scoped_readme(
        cx: &Cx<State>,
        scope: ScopeName,
        package: PackageName,
    ) -> Result<FileContent> {
        readme_content(cx, Some(&scope), &package).await
    }

    #[file("/{package}/README.md")]
    async fn unscoped_readme(cx: &Cx<State>, package: PackageName) -> Result<FileContent> {
        readme_content(cx, None, &package).await
    }

    // ---- Dist-tags directory ----

    #[dir("/@{scope}/{package}/dist-tags")]
    async fn scoped_dist_tags(
        cx: &DirCx<'_, State>,
        scope: ScopeName,
        package: PackageName,
    ) -> Result<Projection> {
        dist_tags_projection(cx, Some(&scope), &package).await
    }

    #[dir("/{package}/dist-tags")]
    async fn unscoped_dist_tags(
        cx: &DirCx<'_, State>,
        package: PackageName,
    ) -> Result<Projection> {
        dist_tags_projection(cx, None, &package).await
    }

    // ---- Versions directory (list of version subdirs) ----

    #[dir("/@{scope}/{package}/versions")]
    async fn scoped_versions(
        cx: &DirCx<'_, State>,
        scope: ScopeName,
        package: PackageName,
    ) -> Result<Projection> {
        versions_projection(cx, Some(&scope), &package).await
    }

    #[dir("/{package}/versions")]
    async fn unscoped_versions(
        cx: &DirCx<'_, State>,
        package: PackageName,
    ) -> Result<Projection> {
        versions_projection(cx, None, &package).await
    }

    // ---- Single version subdirectory ----

    #[dir("/@{scope}/{package}/versions/{version}")]
    async fn scoped_version(
        cx: &DirCx<'_, State>,
        scope: ScopeName,
        package: PackageName,
        version: String,
    ) -> Result<Projection> {
        version_projection(cx, Some(&scope), &package, &version).await
    }

    #[dir("/{package}/versions/{version}")]
    async fn unscoped_version(
        cx: &DirCx<'_, State>,
        package: PackageName,
        version: String,
    ) -> Result<Projection> {
        version_projection(cx, None, &package, &version).await
    }
}

// --- Shared helpers (HTTP + projection building) ---

async fn package_projection(
    cx: &Cx<State>,
    scope: Option<&ScopeName>,
    package: &PackageName,
) -> Result<Projection> {
    let packument = load_packument(cx, scope, package).await?;
    let mut projection = Projection::new();
    projection.file_with_content("name", packument.name.into_bytes());
    if let Some(modified) = packument.modified {
        projection.file_with_content("modified", modified.into_bytes());
    }
    if let Some(latest) = packument.dist_tags.get("latest") {
        projection.file_with_content("latest", latest.clone().into_bytes());
    }
    // dist-tags, versions, README.md are auto-injected as static sibling
    // children by the SDK from the matching #[dir]/#[file] templates.
    projection.page(PageStatus::Exhaustive);
    Ok(projection)
}

async fn readme_content(
    cx: &Cx<State>,
    scope: Option<&ScopeName>,
    package: &PackageName,
) -> Result<FileContent> {
    let packument = npm_client(cx).packument_full(scope, package).await?;
    let bytes = packument.readme.unwrap_or_default().into_bytes();
    Ok(FileContent::bytes(bytes))
}

async fn dist_tags_projection(
    cx: &Cx<State>,
    scope: Option<&ScopeName>,
    package: &PackageName,
) -> Result<Projection> {
    let packument = load_packument(cx, scope, package).await?;
    let mut tags = packument.dist_tags.into_iter().collect::<Vec<_>>();
    tags.sort_by(|left, right| left.0.cmp(&right.0));
    let mut projection = Projection::new();
    for (tag, version) in tags {
        projection.file_with_content(tag, version.into_bytes());
    }
    projection.page(PageStatus::Exhaustive);
    Ok(projection)
}

async fn versions_projection(
    cx: &Cx<State>,
    scope: Option<&ScopeName>,
    package: &PackageName,
) -> Result<Projection> {
    let packument = load_packument(cx, scope, package).await?;
    let mut versions = packument.versions.into_keys().collect::<Vec<_>>();
    sort_versions_desc(&mut versions);
    let mut projection = Projection::new();
    for version in versions {
        projection.dir(version);
    }
    projection.page(PageStatus::Exhaustive);
    Ok(projection)
}

async fn version_projection(
    cx: &Cx<State>,
    scope: Option<&ScopeName>,
    package: &PackageName,
    version: &str,
) -> Result<Projection> {
    let doc = npm_client(cx).version(scope, package, version).await?;

    if doc.version != version {
        return Err(ProviderError::not_found(format!(
            "published version not found: {version}"
        )));
    }

    let mut projection = Projection::new();
    projection.file_with_content("version", doc.version.clone().into_bytes());
    if let Some(deprecated) = doc.deprecated {
        projection.file_with_content("deprecated", deprecated.into_bytes());
    }
    projection.file_with_content("package.json", to_json_bytes(&doc.package_json)?);
    projection.file_with_content(
        "dependencies.json",
        to_json_bytes(&doc.dependencies_json)?,
    );
    if let Some(integrity) = doc.integrity {
        projection.file_with_content("integrity", integrity.into_bytes());
    }
    if let Some(shasum) = doc.shasum {
        projection.file_with_content("shasum", shasum.into_bytes());
    }
    if let Some(unpacked_size) = doc.unpacked_size {
        projection.file_with_content("unpacked-size", unpacked_size.to_string().into_bytes());
    }
    if let Some(file_count) = doc.file_count {
        projection.file_with_content("file-count", file_count.to_string().into_bytes());
    }
    projection.file_with_content("tarball-url", doc.tarball_url.into_bytes());
    projection.file_with_content("signatures.json", to_json_bytes(&doc.signatures_json)?);
    projection.page(PageStatus::Exhaustive);
    Ok(projection)
}

async fn load_packument(
    cx: &Cx<State>,
    scope: Option<&ScopeName>,
    package: &PackageName,
) -> Result<AbbrevPackument> {
    npm_client(cx).packument_abbrev(scope, package).await
}

fn to_json_bytes(value: &serde_json::Value) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(value).map_err(|error| {
        ProviderError::invalid_input(format!("JSON serialize error: {error}"))
    })
}
```

Notes on behavioral parity:

- `package.json` and `signatures.json` can exceed 64 KiB for very
  large packages. `file_with_content` silently rejects bodies larger
  than `MAX_PROJECTED_BYTES` and converts the projection into an
  `invalid_input` error. See "Risks/gotchas" below for the fallback.
- The old `Dir for Package` returned `IdentityKey { namespace:
  "npm.package", id }` and `Dir for Version` returned `IdentityKey {
  namespace: "npm.version", id }`. Identity keys are **gone** in the
  new SDK; the host caches by path and invalidates via
  `EventOutcome`/FUSE notifier. Drop the identity concept; no
  replacement is needed for read-only npm browsing.
- `modified` (from packument) is stored at the package level only. The
  old provider used it as a free-standing file; the new projection
  keeps that.

### Event handling migration

The old npm provider did not emit any `CacheInvalidate*` effects and
did not participate in event-driven invalidation; the migration does
not need an `on_event` handler. The `#[provider(...)]` impl block has
no `on_event` function and that's intentional.

If a future change wants timer-driven packument refresh, add
`async fn on_event(cx: Cx<State>, event: ProviderEvent) ->
Result<EventOutcome>` to the provider impl per the GitHub provider
pattern (`providers/github/src/events.rs`), using
`cx.active_paths(...)` to discover currently-mounted packages and
`outcome.invalidate_prefix(...)` to kick cached listings.

### Remove and create

- Delete nothing physically. `lib.rs`, `provider.rs`, `root.rs`,
  `package.rs` are all rewritten in place.
- `api.rs` and `types.rs` are edited (`api.rs`) or untouched
  (`types.rs`).

No new files are added beyond the four already present plus `api.rs`
and `types.rs`.

## Verification

Run from the worktree root
(`/Users/raul/W/gvfs/.worktrees/providers/npm`):

```bash
# Formatting
cargo fmt --check

# Provider clippy (must include wasm target, per CLAUDE.md)
cargo clippy -p omnifs-provider-npm --target wasm32-wasip2 -- -D warnings

# Provider test compile (WASM tests cannot execute; --no-run only)
cargo test -p omnifs-provider-npm --target wasm32-wasip2 --no-run

# Full provider matrix (dns + github + npm + test)
just check-providers
```

Do not run `just check` as the primary verification: it also runs
host-side tests that are unrelated to this migration. If you do run
it, ensure it still passes after adding npm to the workspace
`members`; a broken provider breaks the provider-side half of `just
check` via its clippy/test phases.

## Risks and gotchas

- **Scoped packages.** Covered above. The two-handler pattern
  (`/@{scope}/{package}` + `/{package}`) is the only supported shape;
  never collapse to `/{pkg}` or `/{*rest}`.
- **Dist-tags like `latest`, `next`, `beta`.** The `dist-tags` dir
  projects arbitrary tags as files whose content is the version
  string. Handler semantics are identical to the old behavior; nothing
  special here beyond sorting for determinism.
- **64 KiB eager-content cap.** `Projection::file_with_content` silently
  drops entries whose bytes exceed `MAX_PROJECTED_BYTES = 64 * 1024`
  and marks the projection as an `invalid_input` error at yield time.
  `package.json` and `signatures.json` can exceed 64 KiB for large
  packages. If this becomes a problem in practice, convert the
  oversized fields to dedicated `#[file("...")]` handlers that fetch
  and return the content as `FileContent::bytes(...)` on demand, and
  drop the `projection.file_with_content(...)` line for that field.
  For the initial migration, keep the eager projection and accept that
  the rare packument over 64 KiB will surface an invalid-input error
  until converted.
- **Large tarballs.** The new SDK file `FileContent::bytes(...)` reads
  the full body into memory; there is no streamed/range variant wired
  through the host runtime today (the SDK enum has `Stream` / `Range`
  variants but the host returns `unimplemented` for them, per
  `crates/omnifs-sdk/src/handler.rs`). The npm migration **does not
  add** a tarball handler. If one is added later
  (`/@{scope}/{package}/versions/{version}/package.tgz`), document the
  memory cost and gate behind a config.
- **Integrity hashes and deprecation metadata.** Preserved as
  `integrity`, `shasum`, `deprecated` sibling files inside each
  `versions/{version}` directory, exactly as before. Deprecated
  versions still appear in `versions/`; the `deprecated` file is only
  present when the registry returned a non-empty deprecation string.
- **HTTP 4xx mapping.** `send_body()` auto-maps 404 to
  `ProviderError::not_found`, 401 to `permission_denied`, 403 to
  `denied`, 429 to `rate_limited`. The old `get_bytes` just bubbled
  the raw error up; this is a net improvement. No code changes
  needed in `api.rs` beyond the `Result` rename.
- **Authorization header.** The old API client did not attach an
  `Authorization` header; the config has no token field. The new
  provider also does not (capabilities declares `bearer-token` but no
  token plumbing exists). If private-registry support is needed
  later, add `auth_token: Option<String>` to `Config` and inject via
  `req.header("Authorization", format!("Bearer {t}"))` in
  `NpmClient::get_bytes`. Out of scope for this migration.
- **`cargo component` metadata.** The `[package.metadata.component]`
  blocks in `providers/npm/Cargo.toml` are vestigial per CLAUDE.md but
  kept for WIT-world documentation. Do not remove.
- **Workspace merge gotchas.** Main's `Cargo.toml` lacks
  `providers/npm` in `members`; adding it is part of this plan. Do
  **not** drop `providers/test` from `members` when editing. The old
  worktree's `Cargo.toml` also lacks the stricter `[workspace.lints]`
  block that landed on main; keep main's lints section intact.
- **Preload idiom.** The old provider did not use `CacheInvalidate*`
  or sibling-file preload APIs (no `Lookup::with_sibling_files` /
  `FileContent::with_sibling_files` callers). The migration therefore
  does not need to introduce new preload calls; relying on
  `Projection::file_with_content` for the package/version directories
  gives the host all the sibling content it needs for free.

---

## Verification

- `cargo fmt --check`
- `cargo clippy -p omnifs-provider-npm --target wasm32-wasip2 -- -D warnings`
- `cargo test -p omnifs-provider-npm --target wasm32-wasip2 --no-run`
- `just check-providers`

All must pass. If `just` is not on PATH, note that in the PR body and run
the equivalent `cargo` commands from the root of this branch's worktree.

## Commit

Conventional:

```
feat(npm): migrate provider to path-first handler SDK
```

Body: one paragraph naming the major structural changes and the base SDK
PRs (#28 `feat/sdk-http-post-support`, #29 `feat/sdk-path-rest-captures`,
#D `feat/sdk-error-constructors`).

## PR

- Title: `feat(npm): migrate provider to path-first handler SDK`
- Body: summary + link to this branch's `PLAN.md` + verification results
  (which cargo commands ran, which passed, which were skipped and why).
