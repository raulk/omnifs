# feat/migrate-ipfs

The ipfs provider worktree (`wip/provider-ipfs-impl`, tip `e1d0b85`, forked from main at `7742e99`) was built against the pre-redesign SDK that used the `mounts!` / `Dir` / `Subtree` / `Effect` model.

## Blocked by

This plan cannot start execution until all three of these have merged into `main`:

- PR #28 `feat/sdk-http-post-support` — https://github.com/raulk/omnifs/pull/28
- PR #29 `feat/sdk-path-rest-captures` — https://github.com/raulk/omnifs/pull/29
- PR TBD `feat/sdk-error-constructors` — error constructor convenience methods

## Execution model

This branch was created off `main` at `6343486`. To execute:

1. `git -C /Users/raul/W/gvfs worktree add /Users/raul/W/gvfs/.worktrees/migrate-ipfs feat/migrate-ipfs`
2. Work in that worktree only.
3. Bring in the provider source from the old worktree at
   `/Users/raul/W/gvfs/.worktrees/providers/ipfs/providers/ipfs/`
   per the "Port provider source" step below.
4. Execute this PLAN.md end-to-end. Corrections in the "Migration
   corrections" section are authoritative over anything in the reference
   body that contradicts them.
5. Run the Verification commands listed near the bottom.
6. Commit on the `feat/migrate-ipfs` branch, push, open PR.


## Port provider source

This branch is off `main` at `6343486`, so there is NO merge from
`wip/provider-ipfs-impl` and NO `git merge main`. The wip branch carries OLD-SDK infrastructure
that must not land here. Only provider-local files come over, file by file,
using `git checkout <old-branch> -- <path>` (this pulls the file contents into
the working tree and index without touching anything else).

### Files to copy verbatim (no touch-ups beyond rust import paths / `ProviderResult` → `Result`)

- `providers/ipfs/src/types.rs`

Bring each over with:

```bash
git checkout wip/provider-ipfs-impl -- providers/ipfs/src/types.rs
```

### Files to copy then touch up

- `providers/ipfs/src/api.rs` — copy over, then apply the corrections in
  the relevant reference-body sections (auth removal, POST rewrites, error
  constructor substitutions).

Bring them in with:

```bash
git checkout wip/provider-ipfs-impl -- providers/ipfs/src/api.rs
```

Then edit in place.

### Files to create fresh (do NOT copy from the wip branch)

- `providers/ipfs/src/lib.rs`
- `providers/ipfs/src/provider.rs`
- `providers/ipfs/src/root.rs`
- `providers/ipfs/src/handlers/ (content, current)`

### Files to DISCARD (do NOT bring to this branch)

- `providers/ipfs/src/tree.rs`
- `providers/ipfs/src/old provider.rs`
- `providers/ipfs/src/old lib.rs`

These are old-SDK artifacts (entity projections, tree walkers, routes tables,
manual http_ext wrappers for auth). The new SDK shape replaces them with
path-first handlers.

### Bring over the provider Cargo.toml

```bash
git checkout wip/provider-ipfs-impl -- providers/ipfs/Cargo.toml
```

Then update its SDK dependency declarations to match `providers/github/Cargo.toml`
on the current `main`. In particular, `omnifs-sdk` must point at the workspace
version and not an old path/git revision.

### Re-register the provider in the workspace

The workspace-level `Cargo.toml` on `main` dropped every non-dns/github/test
provider. Re-add `providers/ipfs` to its `members` array. Example diff:

```toml
[workspace]
members = [
    "crates/cli",
    "crates/host",
    "providers/dns",
    "providers/github",
+   "providers/ipfs",
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

This provider hits only public endpoints and requires no auth injection
from the host. In `capabilities()`:

```rust
Capabilities {
    auth_types: vec![],
    domains: vec!["ipfs.io".to_string(), "dweb.link".to_string(), "gateway.pinata.cloud".to_string(), "(configurable gateway)".to_string()],
    ..Default::default()
}
```

Do NOT carry any `token`, `api_key`, or `oauth_access_token` fields on
`Config` or `State`. Do NOT add an `Authorization` header anywhere in
handler code. If the body of the original plan shows otherwise, it is
superseded by this section.

Domains covered (for host redirect/policy purposes):

  - `ipfs.io`
  - `dweb.link`
  - `gateway.pinata.cloud`
  - `(configurable gateway)`

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

### Rest captures for nested content (ipfs-specific, supersedes the "scope-reduced" caveat in the reference body)

PR #29 (`feat/sdk-path-rest-captures`) added `{*path}` rest captures. The
original plan body contains a section saying ipfs handlers are
scope-reduced to one level below `content/` / `current/` because the SDK
cannot express arbitrary nesting. That caveat is dropped; deep traversal is
now a first-class capability. Use it wherever arbitrary nesting is needed.

```rust
#[file("/_ipfs/{cid}/{*path}")]
async fn cid_file(cx: &Cx<State>, cid: String, path: String) -> Result<FileContent> {
    let gw = cx.state(|s| s.gateway.clone());
    let url = if path.is_empty() {
        format!("{gw}/ipfs/{cid}")
    } else {
        format!("{gw}/ipfs/{cid}/{path}")
    };
    let bytes = cx.http().get(url).send_body().await?;
    Ok(FileContent::bytes(bytes))
}

#[dir("/_ipfs/{cid}/{*path}")]
async fn cid_dir(cx: &Cx<State>, cid: String, path: String) -> Result<Projection> {
    // list UnixFS directory at <cid>/<path>; use the gateway's
    // /api/v0/ls or the same gateway path with `Accept: application/json`
    // depending on the configured gateway's capabilities.
    todo!("delegate to ipfs_api::list(cid, path)")
}
```

Apply the same pattern to `/current/{pointer}/{*path}` so the `current/*`
handlers can also resolve into arbitrarily nested content.

Remove any reference-body text that says:

- "ipfs handlers scope-reduced to one level"
- "requires SDK rest-capture work not yet landed"
- "flatten nested content into top-level files only"

These are all obsolete under PR #29.

---

## Reference body (original MIGRATION_PLAN.md; subordinate to the corrections above)

> The content that follows was written for the old-SDK worktree at
> `/Users/raul/W/gvfs/.worktrees/providers/ipfs/MIGRATION_PLAN.md`.
> Read it for provider-specific shape, path tables, gotchas, and per-file
> migration notes. Wherever a passage conflicts with the corrections above
> (auth handling, POST shape, error constructors, rest captures, destructive
> action for crates-io), the corrections win.

# IPFS provider migration plan (old SDK → new path-first SDK)

## Summary

The ipfs provider worktree (`wip/provider-ipfs-impl`, tip `e1d0b85`, forked
from main at `7742e99`) was built against the pre-redesign SDK that used
the `mounts!` / `Dir` / `Subtree` / `Effect` model. Main has since shipped
commit `6343486` ("redesign provider SDK and host runtime around
path-first handlers and callouts"), which replaced that surface with
free-function `#[dir]` / `#[file]` / `#[subtree]` handlers inside
`#[handlers] impl`, a `Projection` builder, typed `Callout` futures, and
`EventOutcome`-based invalidation. All old surfaces used by the ipfs
provider are gone. The provider sources are untracked in the worktree
(uncommitted working-tree files under `providers/ipfs/`), so the migration
is effectively a rewrite of five files (`lib.rs`, `provider.rs`,
`tree.rs`, `api.rs`, `types.rs`) against the new SDK, preserving the
Kubo-RPC API client and the CID/IPNS newtypes.

Four hard architectural constraints discovered in the new SDK force
scope changes:

1. **Rest captures (`{*rest}`) are NOT supported.** `PathPattern::parse`
   explicitly rejects any `{*...}` segment with `"rest captures are not
   supported"` (crates/omnifs-mount-schema/src/lib.rs:99-102). Handlers
   match fixed-segment-count templates only, with optional single-segment
   prefix captures (`@{resolver}`). Arbitrary-depth nested paths under a
   CID cannot be served from one handler.
2. **`#[subtree]` means git repo handoff, not arbitrary-depth delegation.**
   The new `SubtreeRef` is a `tree_ref: u64` returned from
   `cx.git().open_repo(cache_key, clone_url)` (crates/omnifs-sdk/src/git.rs:17-34).
   There is no generic "hand control of a subtree to the provider"
   escape valve. IPFS CIDs have no backing git repo, so the old
   `Subtree for IpfsCidTree`/`IpnsNameTree` pattern cannot be replicated.
3. **HTTP builder has no `.post()` / `.body()` / `.send_response()`.**
   The new `http::Builder` exposes `.get()` only, and `Request` exposes
   `.header()`, `.send() -> HttpResponse` (errors on status ≥ 400),
   and `.send_body() -> Vec<u8>`. There is no way to inspect the body
   of a non-2xx response from the provider (crates/omnifs-sdk/src/http.rs).
   The worktree's locally modified `crates/omnifs-sdk/src/http.rs`
   (still uncommitted) adds `.post()`, `.body()`, `.send_response()`;
   that patch must be dropped when merging main (take main's version).
4. **Kubo RPC requires POST by the CLI, but accepts GET for all
   read-only verbs** (`ls`, `cat`, `block/stat`, `dag/stat`, `resolve`).
   Migration rewrites the RPC layer to use GET + `.send_body()`.

Net scope delta for the migration:

- Keep: the Kubo RPC JSON models, the `CidText`/`IpnsName` newtypes
  (drop `cid` dep usage for canonicalization only if not ergonomic), the
  configuration shape.
- Change: RPC client uses GET and `send_body`; error mapping gets
  coarser (HTTP status → kind, no Kubo error-body introspection).
- Drop from the first migrated cut: deep nesting inside `/_ipfs/<cid>/`
  and `/_ipns/<name>/current/`. Serve root metadata + one level of
  content listing. Callers that want deep browsing should mount the
  IPFS CID's UnixFS root and re-drill via top-level mount paths or wait
  for SDK rest-capture support (tracked outside this migration in
  `docs/provider-design-ipfs.md`).
- Restructure: mount paths move from `/ipfs/...` and `/ipns/...` to
  `/_ipfs/...` and `/_ipns/...` to match the "metadata prefix" idiom
  used in the github provider (`_issues`, `_prs`, `_repo`). The root
  `/` dir lists `_ipfs` and `_ipns` as static children.

## Current path table (verbatim from worktree `providers/ipfs/src/lib.rs` line 34)

```
omnifs_sdk::mounts! {
    capture cid: crate::types::CidText;
    capture name: crate::types::IpnsName;

    "/" (dir) => Root;
    "/ipfs" (dir) => IpfsIndex;
    "/ipfs/{cid}" (subtree) => IpfsCidTree;
    "/ipns" (dir) => IpnsIndex;
    "/ipns/{name}" (subtree) => IpnsNameTree;
}
```

`IpfsCidTree::{lookup,list,read}` implement an in-provider router over the
tail under `/ipfs/<cid>/` producing these children per CID:

- `cid`, `kind`, `codec`, `block_size`, `dag_size` (metadata files)
- `content/` (directory if UnixFS dir, file if UnixFS file, absent for
  raw/dag codecs)
- arbitrary depth under `content/` via Kubo `ls` recursion

`IpnsNameTree::{lookup,list,read}` implements:

- `resolved_path` (file containing the `resolve` API's returned path)
- `current/` (same tree as `/ipfs/<cid>[/subpath]` rooted at the IPNS
  resolution target, if the target resolves to `/ipfs/...`)

## Target path table (new SDK)

All paths are declared on one handler struct, `crate::root::RootHandlers`,
via `#[dir]` / `#[file]` attributes. No rest captures, no subtree.

```
#[dir("/")]                            root index, static children _ipfs, _ipns
#[dir("/_ipfs")]                       non-enumerable prefix dir
#[dir("/_ipfs/{cid}")]                 per-CID metadata dir; static children
                                       cid, kind, codec, block_size,
                                       dag_size, content (projected from
                                       Projection::file*/Projection::dir)
#[file("/_ipfs/{cid}/cid")]            CID string
#[file("/_ipfs/{cid}/kind")]           directory|file|raw|dag
#[file("/_ipfs/{cid}/codec")]          raw|dag-pb|dag-cbor|libp2p-key|unknown
#[file("/_ipfs/{cid}/block_size")]     decimal block size
#[file("/_ipfs/{cid}/dag_size")]       decimal dag size
#[dir("/_ipfs/{cid}/content")]         UnixFS directory listing (one level)
                                       or error if the CID is a file/raw/dag
#[file("/_ipfs/{cid}/content/{name}")] UnixFS file bytes at depth 1, or
                                       ProviderError::not_a_file if the
                                       link is a directory (deep nesting
                                       not supported in this cut)
#[dir("/_ipns")]                       non-enumerable prefix dir
#[dir("/_ipns/{name}")]                per-name metadata dir; static
                                       children resolved_path, current
#[file("/_ipns/{name}/resolved_path")] resolver's returned /ipfs/... path
#[dir("/_ipns/{name}/current")]        list of direct children of the
                                       IPNS resolution target, when it
                                       resolves to a UnixFS directory;
                                       otherwise ProviderError::not_found
#[file("/_ipns/{name}/current/{leaf}")] direct child file bytes
```

Notes:

- Root handlers do not emit dynamic children for `_ipfs` and `_ipns`
  (Kubo has no "list all CIDs" or "list all IPNS names" API). The
  `root()` handler emits them as static `Projection::dir(...)` entries
  with `PageStatus::Exhaustive`, matching the github provider's
  `RootHandlers::root`.
- `/_ipfs/{cid}` projects the five metadata files eagerly with
  `Projection::file_with_content(...)` so lookups for `cid`, `kind`,
  `codec`, `block_size`, `dag_size` do not trigger a second RPC. One
  fetch of `block/stat` + `dag/stat` + codec inspection happens on
  directory projection and results preload into the host cache.
- `content` is emitted as a `Projection::dir("content")` child only
  when the root CID's content kind is directory or file. When the CID
  resolves to UnixFS file, `content` is projected as a file with
  `file_with_stat` whose size comes from `dag_stat`. When the CID is
  raw or dag, `content` is omitted.
- Deep nesting under `content/` is not supported (no rest capture).
  Users who need it must re-root by mounting `/_ipfs/<cid>` at a
  different host path, or wait for SDK-level rest captures.

## Bring worktree up to main

The worktree is on branch `wip/provider-ipfs-impl`, forked from main at
`7742e99`. Main has advanced to `6343486` with the SDK redesign. The
worktree's committed tip `e1d0b85` made uncommitted IPFS files plus a
local SDK patch that is now obsolete.

Procedure (run from `/Users/raul/W/gvfs/.worktrees/providers/ipfs/`):

1. Save the untracked provider sources so they survive the merge:
   ```
   mkdir -p /tmp/ipfs-migrate
   cp -R providers/ipfs /tmp/ipfs-migrate/
   cp docs/provider-design-ipfs.md /tmp/ipfs-migrate/
   ```

2. Discard the working-tree modifications to `crates/omnifs-sdk/src/http.rs`,
   `crates/host/tests/provider_routes_test.rs`, `Cargo.lock`, `Cargo.toml`,
   `justfile` (these were adding the old `.post/.body/.send_response`
   surface and related glue that is either already on main in a different
   shape or obsolete):
   ```
   git checkout -- crates/omnifs-sdk/src/http.rs
   git checkout -- crates/host/tests/provider_routes_test.rs
   git checkout -- Cargo.lock Cargo.toml justfile
   ```

3. Merge main, taking main's side wholesale for `crates/**` and `wit/**`:
   ```
   git fetch origin
   git merge --no-commit origin/main
   # If conflicts arise in crates/ or wit/, resolve by taking theirs:
   git checkout --theirs crates wit
   git add crates wit
   # Any other conflicts outside those trees: resolve manually.
   git commit -m "chore: merge main into ipfs-provider worktree"
   ```

4. Restore the stashed design doc (the source files will be rewritten
   below, so do not copy them back verbatim):
   ```
   cp /tmp/ipfs-migrate/provider-design-ipfs.md docs/
   ```

5. Recreate `providers/ipfs/` from scratch per the per-file sections
   below.

## SDK cheatsheet (inline, no references)

**Crate-level config**

```rust
// lib.rs
pub(crate) use omnifs_sdk::prelude::Result;

#[derive(Clone)]
pub(crate) struct State { /* fields */ }

#[omnifs_sdk::config]
struct Config {
    #[serde(default = "default_api_base_url")]
    api_base_url: String,
    #[serde(default = "default_ipns_resolve_timeout_secs")]
    ipns_resolve_timeout_secs: u64,
}
```

The `#[omnifs_sdk::config]` macro derives `Serialize + Deserialize` and
wires the provider's `initialize()` entrypoint to deserialize the JSON
bytes the host passes. Do not hand-derive.

**Provider**

```rust
// provider.rs
use omnifs_sdk::prelude::*;
use crate::{Config, State};

#[provider(mounts(crate::root::RootHandlers))]
impl IpfsProvider {
    fn init(config: Config) -> Result<(State, ProviderInfo)> {
        let base = config.api_base_url.trim();
        if base.is_empty() {
            return Err(ProviderError::invalid_input(
                "api_base_url must not be empty",
            ));
        }
        Ok((
            State { config },
            ProviderInfo {
                name: "ipfs-provider".to_string(),
                version: "0.1.0".to_string(),
                description:
                    "Read-only IPFS and IPNS browsing via the Kubo RPC API"
                        .to_string(),
            },
        ))
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: Vec::new(),
            auth_types: Vec::new(),
            max_memory_mb: 64,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 0,
        }
    }
}
```

Multiple handler modules are passed as a comma-separated list inside
`mounts(...)`. For ipfs, one module suffices (`RootHandlers`). If the
later split into separate `ipfs`/`ipns` modules is desired, list them
both in `mounts(...)`.

**Handlers**

```rust
use omnifs_sdk::prelude::*;
use crate::{Result, State};

pub struct RootHandlers;

#[handlers]
impl RootHandlers {
    #[dir("/")]
    fn root(_cx: &DirCx<'_, State>) -> Result<Projection> { /* ... */ }

    #[dir("/_ipfs/{cid}")]
    async fn cid_dir(cx: &DirCx<'_, State>, cid: CidText) -> Result<Projection> {
        /* ... */
    }

    #[file("/_ipfs/{cid}/content/{name}")]
    async fn cid_content_file(
        cx: &Cx<State>,
        cid: CidText,
        name: String,
    ) -> Result<FileContent> {
        /* ... */
    }
}
```

Key points:

- Captures are typed: any `T: FromStr` works. `CidText` and `IpnsName`
  already implement `FromStr` in the current `types.rs`; the macro
  parses the segment via `T::from_str(segment)` at route-match time.
- `DirCx<'_, S>` derefs to `Cx<S>`, so `.http()`, `.git()`, `.state()`,
  `.state_mut()` work on both.
- `DirCx` additionally exposes `.intent()` returning a
  `&DirIntent<'a>` which is one of
  `DirIntent::Lookup { child }`, `DirIntent::List { cursor }`, or
  `DirIntent::ReadProjectedFile { name }`. The host calls the same
  `#[dir]` handler for all three operations; branching on `.intent()`
  is how the github `issue_comments_projection` serves a specific
  projected file read without re-fetching the whole listing. The ipfs
  migration does not need this yet; it only uses the default
  "project everything" shape.
- Handlers may be sync or `async fn`. Async handlers compile to
  `BoxFuture`-producing wrappers the macro synthesizes.

**Projection builder**

```rust
let mut p = Projection::new();
p.dir("content");                              // dir child
p.file("resolved_path");                       // file child, placeholder
                                               // 4 KiB size
p.file_with_stat("blob", FileStat {
    size: NonZeroU64::new(1024).unwrap(),
});                                            // file child with known
                                               // size but no content
p.file_with_content("kind", b"directory");     // eager content, size
                                               // derived from bytes.len();
                                               // max 64 KiB; larger
                                               // contents cause
                                               // p.into_error() to
                                               // surface and handler
                                               // returns ProviderError::
                                               // invalid_input
p.preload("sibling/path", bytes);              // cache-fill content the
                                               // host can serve without
                                               // a second RPC; accumulates
                                               // into the listing's
                                               // preload field
p.preload_many([("a", b"x"), ("b", b"y")]);    // batch
p.page(PageStatus::Exhaustive);                // sibling set is complete
p.page(PageStatus::More(Cursor::Opaque(
    "next-token".to_string(),
)));                                           // more pages exist
p.page(PageStatus::More(Cursor::Page(2)));     // integer cursor
```

**Context**

```rust
let url = cx.state(|s| s.config.api_base_url.clone());
cx.state_mut(|s| s.some_cache.insert(key, value));
let body = cx.http()
    .get(format!("{url}/api/v0/ls?arg={cid}"))
    .header("Accept", "application/json")
    .send_body()
    .await?;                                    // Vec<u8>, errors on
                                                // status >= 400 via
                                                // ProviderError::from_http_status
let resp = cx.http().get(url).send().await?;    // HttpResponse {status,
                                                // headers, body}, also
                                                // errors on status >= 400;
                                                // useful when inspecting
                                                // response headers
                                                // (e.g. ETag)
let repo = cx.git()
    .open_repo("github.com/owner/repo",
               "git@github.com:owner/repo.git")
    .await?;                                    // GitRepoInfo { tree_ref }
let responses = join_all(futures).await;        // Vec<Result<T>>, yields
                                                // all callouts in one
                                                // batch for parallel
                                                // execution
```

**File content returned from `#[file]`**

```rust
Ok(FileContent::bytes(body))
// or, with siblings (less common in ipfs migration):
Ok(FileContent::bytes(primary).with_sibling_files([
    ProjectedFile::new("title", title_bytes),
]))
```

**Errors**

```rust
ProviderError::not_found(message)
ProviderError::not_a_directory(message)
ProviderError::not_a_file(message)
ProviderError::invalid_input(message)
ProviderError::network(message)
ProviderError::timeout(message)
ProviderError::denied(message)
ProviderError::rate_limited(message)
ProviderError::too_large(message)
ProviderError::unimplemented(message)
ProviderError::internal(message)
ProviderError::from_http_status(u16)
ProviderError::from_callout_error(&CalloutError)
```

**Caching model**

Host owns caching. Providers MUST NOT add LRUs or TTLs. The only way
providers influence the cache is via `Projection::preload{,_many}` /
`FileContent::with_sibling_files` / `Lookup::with_sibling_files` /
`EventOutcome::invalidate_{path,prefix}`. Projected file sizes must be
non-zero: use `FileStat { size: NonZeroU64::new(...).unwrap() }` or the
`FileStat::placeholder()` helper.

**Manifest section**

The `#[handlers]` macro emits a `omnifs.provider-manifest.v1` WASM
custom section declaring the handler templates the host introspects at
load time. Duplicate templates across handler types and ambiguous
routes (same precedence, overlapping captures) are rejected with
`ProviderError::invalid_input` at `MountRegistry::validate()` time. The
host calls `validate()` once per provider instance.

## Per-file migration

The five source files are rewritten in full against the new SDK.

### Delete + rewrite: `providers/ipfs/src/lib.rs`

```rust
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

pub(crate) use omnifs_sdk::prelude::Result;

mod api;
mod provider;
mod root;
pub(crate) mod types;

#[derive(Clone)]
pub(crate) struct State {
    pub(crate) config: Config,
}

#[derive(Clone)]
#[omnifs_sdk::config]
pub struct Config {
    #[serde(default = "default_api_base_url")]
    pub(crate) api_base_url: String,
    #[serde(default = "default_ipns_resolve_timeout_secs")]
    pub(crate) ipns_resolve_timeout_secs: u64,
}

fn default_api_base_url() -> String {
    String::from("http://127.0.0.1:5001/api/v0")
}

fn default_ipns_resolve_timeout_secs() -> u64 {
    30
}
```

Notes:

- `mounts!` is gone. The `#[provider(mounts(...))]` attribute on
  `IpfsProvider` in `provider.rs` lists handler modules instead.
- `ProviderResult<T>` alias is replaced with the prelude-re-exported
  `omnifs_sdk::prelude::Result` (alias for
  `core::result::Result<T, ProviderError>`). All handlers return `Result<_>`.
- The `pub(crate) use tree::{IpfsCidTree, ...}` re-export is gone; the
  old `tree.rs` types are deleted entirely.
- Do not hand-derive `Serialize` or `Deserialize` on `Config`; the
  `#[omnifs_sdk::config]` macro handles that.

### Delete + rewrite: `providers/ipfs/src/provider.rs`

```rust
use omnifs_sdk::prelude::*;

use crate::{Config, State};

#[provider(mounts(crate::root::RootHandlers))]
impl IpfsProvider {
    fn init(config: Config) -> Result<(State, ProviderInfo)> {
        let base = config.api_base_url.trim();
        if base.is_empty() {
            return Err(ProviderError::invalid_input(
                "api_base_url must not be empty",
            ));
        }
        Ok((
            State { config },
            ProviderInfo {
                name: "ipfs-provider".to_string(),
                version: "0.1.0".to_string(),
                description:
                    "Read-only IPFS and IPNS browsing via the Kubo RPC API"
                        .to_string(),
            },
        ))
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: Vec::new(),
            auth_types: Vec::new(),
            max_memory_mb: 64,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 0,
        }
    }
}
```

Notes:

- The `init` signature changed to return
  `Result<(State, ProviderInfo)>` (i.e. `core::result::Result<_,
  ProviderError>`) rather than the old
  `Result<_, ProviderError>`. Functionally identical; the alias is
  cleaner.
- No `on_event` handler is added: the ipfs provider has no timer or
  event-driven refresh in the original design. Omit entirely.
- `__mounts` is gone. `#[provider]` consumes the `mounts(...)` list
  directly.

### Delete + rewrite: `providers/ipfs/src/api.rs`

```rust
use omnifs_sdk::Cx;
use omnifs_sdk::prelude::{ProviderError, ProviderErrorKind, Result};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use url::form_urlencoded::Serializer;

use crate::State;
use crate::types::{CidText, IpnsName};

pub(crate) struct IpfsApi<'cx> {
    cx: &'cx Cx<State>,
    base_url: String,
    ipns_resolve_timeout_secs: u64,
}

impl<'cx> IpfsApi<'cx> {
    pub(crate) fn new(cx: &'cx Cx<State>) -> Self {
        let (base_url, ipns_resolve_timeout_secs) = cx.state(|state| {
            (
                state.config.api_base_url.clone(),
                state.config.ipns_resolve_timeout_secs,
            )
        });
        Self {
            cx,
            base_url,
            ipns_resolve_timeout_secs,
        }
    }

    pub(crate) async fn block_stat(&self, cid: &CidText) -> Result<BlockStat> {
        self.rpc_json("block/stat", &[("arg", cid.to_string())]).await
    }

    pub(crate) async fn dag_stat(&self, cid: &CidText) -> Result<DagStat> {
        self.rpc_json("dag/stat", &[("arg", cid.to_string())]).await
    }

    pub(crate) async fn ls(&self, path: &str) -> Result<LsObject> {
        let response: LsResponse = self
            .rpc_json(
                "ls",
                &[
                    ("arg", path.to_string()),
                    ("resolve-type", String::from("true")),
                    ("size", String::from("true")),
                ],
            )
            .await?;
        response.objects.into_iter().next().ok_or_else(|| {
            ProviderError::internal(format!("ls returned no object for {path}"))
        })
    }

    pub(crate) async fn try_ls(&self, path: &str) -> Result<Option<LsObject>> {
        match self.ls(path).await {
            Ok(object) => Ok(Some(object)),
            Err(error) if error.kind() == ProviderErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(crate) async fn cat(&self, path: &str) -> Result<Vec<u8>> {
        self.rpc_bytes(
            "cat",
            &[
                ("arg", path.to_string()),
                ("progress", String::from("false")),
            ],
        )
        .await
    }

    pub(crate) async fn probe_cat(&self, path: &str) -> Result<Option<Vec<u8>>> {
        match self
            .rpc_bytes(
                "cat",
                &[
                    ("arg", path.to_string()),
                    ("length", String::from("1")),
                    ("progress", String::from("false")),
                ],
            )
            .await
        {
            Ok(bytes) => Ok(Some(bytes)),
            // New SDK maps 500-series to Network; a Kubo "is a directory"
            // response also lands here. We treat both non-file conditions
            // as "probe says not a file" rather than error.
            Err(error) if matches!(
                error.kind(),
                ProviderErrorKind::NotAFile
                    | ProviderErrorKind::NotFound
                    | ProviderErrorKind::Network
            ) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(crate) async fn resolve_ipns(&self, name: &IpnsName) -> Result<String> {
        let response: ResolveResponse = self
            .rpc_json(
                "resolve",
                &[
                    ("arg", format!("/ipns/{name}")),
                    ("recursive", String::from("true")),
                    (
                        "dht-timeout",
                        format!("{}s", self.ipns_resolve_timeout_secs),
                    ),
                ],
            )
            .await?;
        Ok(response.path)
    }

    // ---------------------------------------------------------------
    // Transport
    // ---------------------------------------------------------------

    async fn rpc_json<T>(&self, cmd: &str, query: &[(&str, String)]) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let body = self.rpc_bytes(cmd, query).await?;
        serde_json::from_slice(&body).map_err(|error| {
            ProviderError::invalid_input(format!(
                "{cmd} returned invalid JSON: {error}"
            ))
        })
    }

    // Kubo RPC accepts GET for all read-only verbs we use here
    // (ls, cat, block/stat, dag/stat, resolve). The new SDK's HTTP
    // builder only offers .get() / .send() / .send_body() and
    // auto-errors on status >= 400 via ProviderError::from_http_status,
    // so fine-grained Kubo error-body introspection is not available.
    // Status codes still map sensibly: 404 -> not_found,
    // 400 -> invalid_input, 5xx -> network.
    async fn rpc_bytes(&self, cmd: &str, query: &[(&str, String)]) -> Result<Vec<u8>> {
        let url = build_rpc_url(&self.base_url, cmd, query);
        self.cx.http().get(url).send_body().await
    }
}

// ---------------------------------------------------------------
// Response models (unchanged from the old api.rs)
// ---------------------------------------------------------------

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct BlockStat {
    #[serde(rename = "Size", deserialize_with = "deserialize_u64")]
    pub(crate) size: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct DagStat {
    #[serde(
        rename = "TotalSize",
        default,
        deserialize_with = "deserialize_optional_u64"
    )]
    total_size: Option<u64>,
    #[serde(rename = "DagStats", default)]
    dag_stats: Vec<DagStatEntry>,
}

impl DagStat {
    pub(crate) fn total_size(&self) -> u64 {
        self.total_size
            .or_else(|| self.dag_stats.first().map(|entry| entry.size))
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, Deserialize)]
struct DagStatEntry {
    #[serde(rename = "Size", deserialize_with = "deserialize_u64")]
    size: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct LsResponse {
    #[serde(rename = "Objects", default)]
    objects: Vec<LsObject>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct LsObject {
    #[serde(rename = "Links", default)]
    pub(crate) links: Vec<LsLink>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct LsLink {
    #[serde(rename = "Name")]
    pub(crate) name: String,
    #[serde(rename = "Size", deserialize_with = "deserialize_u64")]
    pub(crate) size: u64,
    #[serde(rename = "Type", deserialize_with = "deserialize_i32")]
    pub(crate) kind: i32,
}

#[derive(Clone, Debug, Deserialize)]
struct ResolveResponse {
    #[serde(rename = "Path")]
    path: String,
}

fn build_rpc_url(base_url: &str, cmd: &str, query: &[(&str, String)]) -> String {
    let mut url = format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        cmd.trim_start_matches('/')
    );
    if query.is_empty() {
        return url;
    }

    let mut serializer = Serializer::new(String::new());
    for (name, value) in query {
        serializer.append_pair(name, value);
    }
    url.push('?');
    url.push_str(&serializer.finish());
    url
}

fn deserialize_u64<'de, D>(deserializer: D) -> core::result::Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        U64(u64),
        I64(i64),
        String(String),
    }

    match Value::deserialize(deserializer)? {
        Value::U64(value) => Ok(value),
        Value::I64(value) => u64::try_from(value).map_err(serde::de::Error::custom),
        Value::String(value) => value.parse().map_err(serde::de::Error::custom),
    }
}

fn deserialize_optional_u64<'de, D>(
    deserializer: D,
) -> core::result::Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<serde_json::Value>::deserialize(deserializer)?.map_or(Ok(None), |value| {
        parse_json_u64(value)
            .map(Some)
            .map_err(serde::de::Error::custom)
    })
}

fn deserialize_i32<'de, D>(deserializer: D) -> core::result::Result<i32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        I32(i32),
        I64(i64),
        String(String),
    }

    match Value::deserialize(deserializer)? {
        Value::I32(value) => Ok(value),
        Value::I64(value) => i32::try_from(value).map_err(serde::de::Error::custom),
        Value::String(value) => value.parse().map_err(serde::de::Error::custom),
    }
}

fn parse_json_u64(value: serde_json::Value) -> core::result::Result<u64, String> {
    match value {
        serde_json::Value::Number(number) => number
            .as_u64()
            .ok_or_else(|| String::from("expected u64 number")),
        serde_json::Value::String(value) => value.parse::<u64>().map_err(|error| error.to_string()),
        other => Err(format!("expected u64-compatible value, got {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_rpc_url_repeats_arg_pairs() {
        let url = build_rpc_url(
            "https://kubo.test/api/v0",
            "resolve",
            &[
                ("arg", "/ipns/example.com".to_string()),
                ("arg", "/ipfs/bafy".to_string()),
                ("recursive", "true".to_string()),
            ],
        );

        assert_eq!(
            url,
            "https://kubo.test/api/v0/resolve?arg=%2Fipns%2Fexample.com&arg=%2Fipfs%2Fbafy&recursive=true"
        );
    }
}
```

What changed from the old file:

- `rpc()`/`rpc_bytes()` collapsed: POST + `send_response()` became GET
  + `send_body()`.
- The `map_kubo_error` / `kubo_error_message` helpers are deleted.
  Error classification now relies on `ProviderError::from_http_status`
  (applied automatically inside `send_body()`). The old test
  `kubo_error_mapping_recovers_not_found_from_rpc_500` is removed with
  its helper.
- `ProviderResult<T>` → `Result<T>` (re-exported alias).
- `HttpResponse` import deleted (no longer used here).

### Delete + rewrite: `providers/ipfs/src/tree.rs`

The entire old `tree.rs` is deleted. The routing/projection logic
moves into `providers/ipfs/src/root.rs` as free functions registered
via `#[handlers]`. Nothing from `tree.rs` survives directly; the
helpers (`inspect_cid_root`, `classify_root`, `codec_name`,
`link_to_entry`, etc.) are inlined or adapted into the new root.rs.

### New file: `providers/ipfs/src/root.rs`

```rust
use std::num::NonZeroU64;

use omnifs_sdk::prelude::*;

use crate::api::{IpfsApi, LsLink};
use crate::types::{CidText, IpnsName};
use crate::{Result, State};

pub struct RootHandlers;

#[handlers]
impl RootHandlers {
    // ---------------------------------------------------------------
    // Root
    // ---------------------------------------------------------------

    #[dir("/")]
    fn root(_cx: &DirCx<'_, State>) -> Result<Projection> {
        // _ipfs and _ipns are static directories that cannot be
        // enumerated (no "list all CIDs / all IPNS names" API).
        let mut projection = Projection::new();
        projection.dir("_ipfs");
        projection.dir("_ipns");
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[dir("/_ipfs")]
    fn ipfs_index(_cx: &DirCx<'_, State>) -> Result<Projection> {
        // Non-enumerable prefix: users navigate by CID.
        let mut projection = Projection::new();
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[dir("/_ipns")]
    fn ipns_index(_cx: &DirCx<'_, State>) -> Result<Projection> {
        // Non-enumerable prefix: users navigate by IPNS name.
        let mut projection = Projection::new();
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    // ---------------------------------------------------------------
    // /_ipfs/{cid}
    // ---------------------------------------------------------------

    #[dir("/_ipfs/{cid}")]
    async fn cid_dir(cx: &DirCx<'_, State>, cid: CidText) -> Result<Projection> {
        let summary = inspect_cid_root(&cid, cx).await?;
        let mut projection = Projection::new();

        // Metadata files; project eagerly so a later read is served
        // from the host cache.
        projection.file_with_content("cid", cid.to_string().into_bytes());
        projection.file_with_content("kind", summary.kind_label().as_bytes().to_vec());
        projection.file_with_content("codec", codec_name(&cid).as_bytes().to_vec());
        projection.file_with_content(
            "block_size",
            summary.block_size.to_string().into_bytes(),
        );
        projection.file_with_content(
            "dag_size",
            summary.dag_size.to_string().into_bytes(),
        );

        match summary.content_kind() {
            Some(ContentKind::Directory) => projection.dir("content"),
            Some(ContentKind::File) => {
                let stat = nonzero_size(summary.dag_size);
                projection.file_with_stat("content", stat);
            },
            None => { /* raw or dag: content is absent */ },
        }

        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    // The five metadata files are served as file handlers so direct
    // reads at those paths still hit the provider if the host cache
    // is cold. Handlers fetch the same summary and serve the exact
    // field. This duplicates work when the dir handler's projection
    // has already preloaded them; the host de-dups via the cache.

    #[file("/_ipfs/{cid}/cid")]
    async fn cid_field_cid(_cx: &Cx<State>, cid: CidText) -> Result<FileContent> {
        Ok(FileContent::bytes(cid.to_string().into_bytes()))
    }

    #[file("/_ipfs/{cid}/kind")]
    async fn cid_field_kind(cx: &Cx<State>, cid: CidText) -> Result<FileContent> {
        let summary = inspect_cid_root(&cid, cx).await?;
        Ok(FileContent::bytes(summary.kind_label().as_bytes().to_vec()))
    }

    #[file("/_ipfs/{cid}/codec")]
    async fn cid_field_codec(_cx: &Cx<State>, cid: CidText) -> Result<FileContent> {
        Ok(FileContent::bytes(codec_name(&cid).as_bytes().to_vec()))
    }

    #[file("/_ipfs/{cid}/block_size")]
    async fn cid_field_block_size(cx: &Cx<State>, cid: CidText) -> Result<FileContent> {
        let summary = inspect_cid_root(&cid, cx).await?;
        Ok(FileContent::bytes(summary.block_size.to_string().into_bytes()))
    }

    #[file("/_ipfs/{cid}/dag_size")]
    async fn cid_field_dag_size(cx: &Cx<State>, cid: CidText) -> Result<FileContent> {
        let summary = inspect_cid_root(&cid, cx).await?;
        Ok(FileContent::bytes(summary.dag_size.to_string().into_bytes()))
    }

    // ---------------------------------------------------------------
    // /_ipfs/{cid}/content and its direct children
    // ---------------------------------------------------------------

    #[dir("/_ipfs/{cid}/content")]
    async fn cid_content_dir(
        cx: &DirCx<'_, State>,
        cid: CidText,
    ) -> Result<Projection> {
        let upstream = format!("/ipfs/{cid}");
        let api = IpfsApi::new(cx);
        let Some(object) = api.try_ls(&upstream).await? else {
            return Err(ProviderError::not_a_directory(format!(
                "CID {cid} does not resolve to a UnixFS directory"
            )));
        };
        let mut projection = Projection::new();
        for link in object.links {
            emit_link(&mut projection, &link);
        }
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    // Direct-child file read. Deep nesting is not supported in this cut:
    // the capture grammar does not allow rest captures, so a single
    // `{name}` segment only matches one level below `content`.
    #[file("/_ipfs/{cid}/content/{name}")]
    async fn cid_content_file(
        cx: &Cx<State>,
        cid: CidText,
        name: String,
    ) -> Result<FileContent> {
        let upstream = format!("/ipfs/{cid}/{name}");
        let bytes = IpfsApi::new(cx).cat(&upstream).await?;
        Ok(FileContent::bytes(bytes))
    }

    // ---------------------------------------------------------------
    // /_ipns/{name}
    // ---------------------------------------------------------------

    #[dir("/_ipns/{name}")]
    async fn ipns_dir(cx: &DirCx<'_, State>, name: IpnsName) -> Result<Projection> {
        let resolved_path = IpfsApi::new(cx).resolve_ipns(&name).await?;
        let mut projection = Projection::new();
        projection
            .file_with_content("resolved_path", resolved_path.clone().into_bytes());

        // `current` is only projected when resolution lands on /ipfs/<cid>[/subpath]
        // whose target is a UnixFS directory. File targets still get a
        // `current` file entry; raw/dag targets omit `current`.
        if let Some(target) = parse_resolved_ipfs_target(&resolved_path) {
            let api = IpfsApi::new(cx);
            let upstream = ipfs_path(&target.root, &target.subpath);
            match classify_content(&upstream, &api).await? {
                Some(ContentKind::Directory) => projection.dir("current"),
                Some(ContentKind::File) => {
                    projection.file_with_stat("current", FileStat::placeholder());
                },
                None => {},
            }
        }
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[file("/_ipns/{name}/resolved_path")]
    async fn ipns_resolved_path(cx: &Cx<State>, name: IpnsName) -> Result<FileContent> {
        let path = IpfsApi::new(cx).resolve_ipns(&name).await?;
        Ok(FileContent::bytes(path.into_bytes()))
    }

    #[dir("/_ipns/{name}/current")]
    async fn ipns_current_dir(
        cx: &DirCx<'_, State>,
        name: IpnsName,
    ) -> Result<Projection> {
        let resolved_path = IpfsApi::new(cx).resolve_ipns(&name).await?;
        let Some(target) = parse_resolved_ipfs_target(&resolved_path) else {
            return Err(ProviderError::not_found(format!(
                "IPNS name {name} did not resolve to an /ipfs/... path"
            )));
        };
        let upstream = ipfs_path(&target.root, &target.subpath);
        let api = IpfsApi::new(cx);
        let Some(object) = api.try_ls(&upstream).await? else {
            return Err(ProviderError::not_a_directory(format!(
                "IPNS name {name} does not resolve to a UnixFS directory"
            )));
        };
        let mut projection = Projection::new();
        for link in object.links {
            emit_link(&mut projection, &link);
        }
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }

    #[file("/_ipns/{name}/current/{leaf}")]
    async fn ipns_current_file(
        cx: &Cx<State>,
        name: IpnsName,
        leaf: String,
    ) -> Result<FileContent> {
        let resolved_path = IpfsApi::new(cx).resolve_ipns(&name).await?;
        let target = parse_resolved_ipfs_target(&resolved_path)
            .ok_or_else(|| ProviderError::not_found(format!(
                "IPNS name {name} did not resolve to an /ipfs/... path"
            )))?;
        let subpath = join_subpath(&target.subpath, &leaf);
        let upstream = ipfs_path(&target.root, &subpath);
        let bytes = IpfsApi::new(cx).cat(&upstream).await?;
        Ok(FileContent::bytes(bytes))
    }
}

// -------------------------------------------------------------------
// Helpers (ported from the old tree.rs)
// -------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ContentKind {
    Directory,
    File,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RootKind {
    Directory,
    File,
    Raw,
    Dag,
}

struct CidSummary {
    block_size: u64,
    dag_size: u64,
    kind: RootKind,
}

impl CidSummary {
    fn kind_label(&self) -> &'static str {
        match self.kind {
            RootKind::Directory => "directory",
            RootKind::File => "file",
            RootKind::Raw => "raw",
            RootKind::Dag => "dag",
        }
    }

    fn content_kind(&self) -> Option<ContentKind> {
        match self.kind {
            RootKind::Directory => Some(ContentKind::Directory),
            RootKind::File => Some(ContentKind::File),
            RootKind::Raw | RootKind::Dag => None,
        }
    }
}

struct ResolvedContentTarget {
    root: CidText,
    subpath: String,
}

async fn inspect_cid_root(cid: &CidText, cx: &Cx<State>) -> Result<CidSummary> {
    let api = IpfsApi::new(cx);
    let block_stat = api.block_stat(cid).await?;
    let dag_stat = api.dag_stat(cid).await?;
    let root_path = format!("/ipfs/{cid}");
    let kind = classify_root(cid, &root_path, &api).await?;
    Ok(CidSummary {
        block_size: block_stat.size,
        dag_size: dag_stat.total_size(),
        kind,
    })
}

async fn classify_root(
    cid: &CidText,
    root_path: &str,
    api: &IpfsApi<'_>,
) -> Result<RootKind> {
    if cid.codec() == 0x55 {
        return Ok(RootKind::Raw);
    }
    if api.probe_cat(root_path).await?.is_some() {
        return Ok(RootKind::File);
    }
    if api.try_ls(root_path).await?.is_some() {
        return Ok(RootKind::Directory);
    }
    Ok(RootKind::Dag)
}

async fn classify_content(
    upstream: &str,
    api: &IpfsApi<'_>,
) -> Result<Option<ContentKind>> {
    if api.probe_cat(upstream).await?.is_some() {
        return Ok(Some(ContentKind::File));
    }
    if api.try_ls(upstream).await?.is_some() {
        return Ok(Some(ContentKind::Directory));
    }
    Ok(None)
}

fn emit_link(projection: &mut Projection, link: &LsLink) {
    if is_directory_link(link.kind) {
        projection.dir(link.name.clone());
    } else {
        let stat = nonzero_size(link.size);
        projection.file_with_stat(link.name.clone(), stat);
    }
}

fn is_directory_link(kind: i32) -> bool {
    matches!(kind, 1 | 5)
}

fn parse_resolved_ipfs_target(path: &str) -> Option<ResolvedContentTarget> {
    let rest = path.strip_prefix("/ipfs/")?;
    let (cid, subpath) = rest
        .split_once('/')
        .map_or((rest, ""), |(cid, subpath)| (cid, subpath));
    Some(ResolvedContentTarget {
        root: cid.parse().ok()?,
        subpath: subpath.to_string(),
    })
}

fn ipfs_path(root: &CidText, subpath: &str) -> String {
    if subpath.is_empty() {
        format!("/ipfs/{root}")
    } else {
        format!("/ipfs/{root}/{subpath}")
    }
}

fn join_subpath(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        suffix.to_string()
    } else if suffix.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}/{suffix}")
    }
}

fn codec_name(cid: &CidText) -> &'static str {
    match cid.codec() {
        0x55 => "raw",
        0x70 => "dag-pb",
        0x71 => "dag-cbor",
        0x72 => "libp2p-key",
        _ => "unknown",
    }
}

fn nonzero_size(size: u64) -> FileStat {
    FileStat {
        size: NonZeroU64::new(size).unwrap_or_else(|| NonZeroU64::new(4096).unwrap()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_resolved_target_extracts_root_and_subpath() {
        let parsed = parse_resolved_ipfs_target(
            "/ipfs/bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi/docs/readme.md",
        )
        .unwrap();

        assert_eq!(
            parsed.root.to_string(),
            "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi"
        );
        assert_eq!(parsed.subpath, "docs/readme.md");
    }

    #[test]
    fn join_subpath_avoids_duplicate_separators() {
        assert_eq!(join_subpath("", "docs"), "docs");
        assert_eq!(join_subpath("docs", ""), "docs");
        assert_eq!(join_subpath("docs", "readme.md"), "docs/readme.md");
    }
}
```

What changed:

- The old `Dir`/`Subtree` traits are replaced by path-attributed
  free functions collected via `#[handlers] impl RootHandlers`.
- The old `IpfsCidTree::lookup/list/read` in-provider router is gone;
  each path shape gets its own handler.
- `Entry::{dir,file}`, `EntryStat`, `Page<Entry>`, `RelPath`,
  `MountPath`, `FileBytes` are gone. Files come out as
  `Projection::file`/`file_with_stat`/`file_with_content`;
  listings are terminated with `projection.page(PageStatus::Exhaustive)`.
- The `metadata_file_name` / `content_subpath` / `split_parent_and_name`
  helpers are gone (no in-provider routing needed).
- Deep content nesting removed; see "Risks/gotchas" below.

### Keep (minor touch-ups): `providers/ipfs/src/types.rs`

The old `types.rs` is kept as-is functionally. The `CidText`/`IpnsName`
newtypes already implement `FromStr` with the signatures the new
`#[dir]`/`#[file]` macros expect. Verify the following after the
migration merges:

- `CidText::FromStr` returns `Result<Self, ()>`: keep, but consider
  upgrading `Err` to a descriptive type to improve handler miss
  messages. The macro surfaces `FromStr::Err` via `Display` in the
  404-path; a unit-error yields a blank string. Minor cosmetic fix:
  replace `type Err = ();` with a newtype error struct implementing
  `Display`. This is optional for a working migration.
- `IpnsName::FromStr`: same.
- Tests in the old file compile under `#[cfg(test)]` for the native
  target only (they don't touch WASM-specific surface). Keep them.

If `cid` crate usage becomes awkward (it pulls in `multibase` and
related transitive deps), consider switching to a regex-based CID
validator. Optional; do not block the migration on it.

### Keep: `providers/ipfs/src/lib.rs` test dependencies

None. The old `lib.rs` had no tests.

## Event handling migration

The old IPFS provider did not implement any event handlers. The new
SDK's `#[provider]` attribute makes `on_event` optional, and its
`refresh_interval_secs: 0` capability disables timer ticks. Nothing to
migrate here; omit `on_event` entirely.

All OLD effects that the old model might have used (none in ipfs
today) map as follows for future reference:

- `CacheInvalidatePath` / `CacheInvalidatePrefix` effects →
  `EventOutcome::invalidate_path(...)` /
  `EventOutcome::invalidate_prefix(...)` returned from an `on_event`
  handler. Not used in ipfs.
- `GitListTree`, `GitReadBlob`, `GitHeadRef`, `GitListCachedRepos`
  effects → all gone. The only git callout is
  `cx.git().open_repo(cache_key, clone_url)` which returns a
  `GitRepoInfo { tree_ref: u64 }` for use with `SubtreeRef`. Not used
  in ipfs (no git-backed trees).
- All `Effect`/`SingleEffect` variants → `Callout::Fetch` and
  `Callout::GitOpenRepo` only. Provider code does not name these
  directly; they are wrapped by `cx.http()` and `cx.git()` builders.

## Cargo.toml changes

### Provider manifest: `providers/ipfs/Cargo.toml`

Keep the existing structure; only dependency edits are required.

```toml
[package]
name = "omnifs-provider-ipfs"
version = "0.1.0"
edition = "2024"
description = "OmnIFS provider for read-only IPFS and IPNS browsing via Kubo RPC"
license = "MIT OR Apache-2.0"
repository = "https://github.com/raulk/omnifs"
homepage = "https://github.com/raulk/omnifs"

[lib]
crate-type = ["cdylib", "lib"]

[dependencies]
cid = "0.11"
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

Notes:

- No new deps needed.
- Keep the `[package.metadata.component]` stanzas per project CLAUDE.md
  ("vestigial from `cargo component build`...but kept for documentation
  of the WIT world mapping").
- The `cid = "0.11"` dep is retained for canonical CID parsing in
  `types.rs`. If dependency-bloat becomes a concern after first
  compile, consider dropping it and hand-validating CIDs with a
  base32-lower regex, but do not do so speculatively.

### Workspace manifest: `Cargo.toml` (worktree root)

After merging main, the workspace `members` list has to include
`"providers/ipfs"`. Main's `Cargo.toml` currently lists:

```toml
members = [
    "crates/cli",
    "crates/host",
    "providers/github",
    "providers/dns",
    "providers/test",
]
```

Edit to:

```toml
members = [
    "crates/cli",
    "crates/host",
    "providers/github",
    "providers/dns",
    "providers/ipfs",
    "providers/test",
]
```

Do not touch `default-members`, `[workspace.dependencies]`, or
`[workspace.lints.*]`. Do not add `cid`, `url`, or
`serde_json`-with-flags at workspace level; the provider pulls them
directly.

## Verification

After merging main and rewriting the five files, run from the worktree
root:

```
cargo fmt --check
cargo clippy -p omnifs-provider-ipfs --target wasm32-wasip2 -- -D warnings
cargo test  -p omnifs-provider-ipfs --target wasm32-wasip2 --no-run
just check-providers
```

Expected outcomes:

- `cargo fmt --check`: no diffs. If rustfmt reports issues, run
  `cargo fmt -p omnifs-provider-ipfs` and recommit.
- `cargo clippy ... -D warnings`: no warnings. The lints block in the
  provider Cargo.toml already allows the noisy pedantic checks
  (`module_name_repetitions`, `wildcard_imports`, etc.).
- `cargo test ... --no-run`: compiles all `#[cfg(test)]` modules for
  wasm32-wasip2. WASM tests do not execute in the host test harness;
  `--no-run` is mandatory per project CLAUDE.md.
- `just check-providers`: the project's recipe for fmt + clippy +
  test-compile across all providers. Must pass before declaring the
  migration done.

If the `root.rs` test module's `parse_resolved_target_extracts_root_and_subpath`
asserts fail at link time because `cid` crate transitive deps pull in
something incompatible with wasm32-wasip2, move the test to
`#[cfg(all(test, not(target_arch = "wasm32")))]`. Do not delete it; the
logic under test is a pure string function.

## Risks and gotchas

**Rest captures are unavailable.** The new SDK's `PathPattern::parse`
explicitly rejects `{*rest}`:

```
if raw.starts_with("{*") {
    return Err(pattern_error(format!(
        "rest captures are not supported in {raw:?}"
    )));
}
```

This breaks the old `Subtree` model's ability to serve arbitrary-depth
tails under `/ipfs/<cid>/...` and `/ipns/<name>/current/...`. The
migration restricts browsing to one level below `content/` and
`current/`. Deeper directory browsing requires either SDK work (add
rest-capture grammar) or a host-side change (auto-register per-depth
handlers up to a bound). Neither is in scope for this migration.
Surface this to the user before merging: the ipfs provider as migrated
is strictly shallower than the design doc implies.

**`#[subtree]` is git-only.** `SubtreeRef::new(tree_ref: u64)` is a
handle to a git repo cloned by the host and bind-mounted at the
subtree path. There is no equivalent for IPFS. Do not attempt to
model IPFS CIDs as subtrees; they must be projected through `#[dir]`
and `#[file]` handlers.

**CID validation.** `CidText::from_str` canonicalizes to CIDv1
base32-lower. Any CIDv0 input round-trips to CIDv1. Keep this in mind
when debugging: a user-supplied CIDv0 will appear in logs and error
messages as its CIDv1 form. Error messages should mention this to
avoid confusion.

**dag-pb vs UnixFS vs raw vs dag-cbor.** The `kind` field distinguishes
UnixFS directory / UnixFS file / raw block / opaque DAG. Only UnixFS
dirs project a directory `content/`; UnixFS files project a file
`content`; raw and dag targets omit `content`. Raw/dag traversal is
explicitly out of scope for this migration.

**Large files exceed the 64 KiB eager preload limit.** `Projection::file_with_content`
rejects content > `MAX_PROJECTED_BYTES = 64 * 1024`. Use it only for
the short metadata fields. `FileContent::bytes` from a `#[file]`
handler has no such limit, so large UnixFS files returned from
`cat` pass through unbounded. The host may still apply its own caps
during transport; keep an eye on Docker-compose logs for payload
rejections.

**IPNS record mutability.** IPNS resolution results can change. The
host caches resolutions by path and currently has no time-based
invalidation, so a stale `resolved_path` may persist. Once SDK-level
event handling is understood for ipfs (not this migration), add an
`on_event` handler with `refresh_interval_secs: N` that emits
`EventOutcome::invalidate_prefix("_ipns/<name>")`. Out of scope here.

**Gateway timeouts.** Kubo's `resolve` can hang when the DHT is
unreachable. The provider config exposes `ipns_resolve_timeout_secs`
which is passed as the `dht-timeout` query parameter. If the new SDK
imposes a global HTTP timeout shorter than that, resolution requests
may fail before Kubo returns. The host-level timeout lives in
`crates/host/src/runtime/executor.rs` and is not configurable from
the provider side. If this becomes a problem in practice, escalate
to the host for a per-callout deadline.

**`send_body()` swallows response bodies on errors.** The old
`rpc_bytes` helper inspected Kubo's `{"Message": "..."}` JSON body on
HTTP 500 to distinguish "not found" / "is a directory" / "not a
directory" from opaque server errors. The new SDK loses that level of
detail; all 5xx responses become `ProviderError::network(...)` with
the bare HTTP status. User-facing error messages will read
`HTTP 500` instead of `no link named "missing" under bafy...`. This is
acceptable for the first migrated cut but should be documented in
`docs/provider-design-ipfs.md` as a known regression.

**Deep-nested file access is a hole.** A user mounting a UnixFS
directory with subdirectories will see the top-level listing work but
`ls _ipfs/<cid>/content/subdir` will fail with `path not found`
because no handler matches `/_ipfs/{cid}/content/{subdir}/...`. This
is the rest-capture blocker surfaced above. Do not paper over it with
an ad-hoc depth-N handler sequence (e.g. one handler per
`/_ipfs/{cid}/content/{a}/{b}/{c}`); that invites a maintenance
nightmare and still imposes an arbitrary limit.

**`unsafe_code = "allow"` in provider lints.** Project convention; the
macros may emit `unsafe` for `wit_bindgen`-generated code. Keep.

---

## Verification

- `cargo fmt --check`
- `cargo clippy -p omnifs-provider-ipfs --target wasm32-wasip2 -- -D warnings`
- `cargo test -p omnifs-provider-ipfs --target wasm32-wasip2 --no-run`
- `just check-providers`

All must pass. If `just` is not on PATH, note that in the PR body and run
the equivalent `cargo` commands from the root of this branch's worktree.

## Commit

Conventional:

```
feat(ipfs): migrate provider to path-first handler SDK
```

Body: one paragraph naming the major structural changes and the base SDK
PRs (#28 `feat/sdk-http-post-support`, #29 `feat/sdk-path-rest-captures`,
#D `feat/sdk-error-constructors`).

## PR

- Title: `feat(ipfs): migrate provider to path-first handler SDK`
- Body: summary + link to this branch's `PLAN.md` + verification results
  (which cargo commands ran, which passed, which were skipped and why).
