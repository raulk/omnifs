# feat/migrate-huggingface

This worktree was forked at `7742e99` and has a working but pre-redesign provider (`api.rs`, `entities/`, `events.rs`, `http_ext.rs`, `lib.rs`, `provider.rs`, `types.rs`, ~3000 LoC).

## Blocked by

This plan cannot start execution until both of these have merged into `main`:

- PR #28 `feat/sdk-http-post-support` — https://github.com/raulk/omnifs/pull/28
- PR #29 `feat/sdk-path-rest-captures` — https://github.com/raulk/omnifs/pull/29

Note: `ProviderError::rate_limited` / `::permission_denied` / `::version_mismatch`
constructors are already on `main` (landed with the #27 refactor). No separate PR
is needed.

## Execution model

This branch was created off `main` at `6343486`. To execute:

1. `git -C /Users/raul/W/gvfs worktree add /Users/raul/W/gvfs/.worktrees/migrate-huggingface feat/migrate-huggingface`
2. Work in that worktree only.
3. Bring in the provider source from the old worktree at
   `/Users/raul/W/gvfs/.worktrees/providers/huggingface/providers/huggingface/`
   per the "Port provider source" step below.
4. Execute this PLAN.md end-to-end. Corrections in the "Migration
   corrections" section are authoritative over anything in the reference
   body that contradicts them.
5. Run the Verification commands listed near the bottom.
6. Commit on the `feat/migrate-huggingface` branch, push, open PR.


## Port provider source

This branch is off `main` at `6343486`, so there is NO merge from
`wip/provider-huggingface-impl` and NO `git merge main`. The wip branch carries OLD-SDK infrastructure
that must not land here. Only provider-local files come over, file by file,
using `git checkout <old-branch> -- <path>` (this pulls the file contents into
the working tree and index without touching anything else).

### Files to copy verbatim (no touch-ups beyond rust import paths / `ProviderResult` → `Result`)

- `providers/huggingface/src/types.rs`

Bring each over with:

```bash
git checkout wip/provider-huggingface-impl -- providers/huggingface/src/types.rs
```

### Files to copy then touch up

- `providers/huggingface/src/api.rs` — copy over, then apply the corrections in
  the relevant reference-body sections (auth removal, POST rewrites, error
  constructor substitutions).
- `providers/huggingface/src/events.rs` — copy over, then apply the corrections in
  the relevant reference-body sections (auth removal, POST rewrites, error
  constructor substitutions).

Bring them in with:

```bash
git checkout wip/provider-huggingface-impl -- providers/huggingface/src/api.rs
git checkout wip/provider-huggingface-impl -- providers/huggingface/src/events.rs
```

Then edit in place.

### Files to create fresh (do NOT copy from the wip branch)

- `providers/huggingface/src/lib.rs`
- `providers/huggingface/src/provider.rs`
- `providers/huggingface/src/root.rs`
- `providers/huggingface/src/handlers/ (models, datasets, spaces, users, orgs)`

### Files to DISCARD (do NOT bring to this branch)

- `providers/huggingface/src/http_ext.rs`
- `providers/huggingface/src/entities/ (entire folder)`
- `providers/huggingface/src/old provider.rs`
- `providers/huggingface/src/old lib.rs`

These are old-SDK artifacts (entity projections, tree walkers, routes tables,
manual http_ext wrappers for auth). The new SDK shape replaces them with
path-first handlers.

### Bring over the provider Cargo.toml

```bash
git checkout wip/provider-huggingface-impl -- providers/huggingface/Cargo.toml
```

Then update its SDK dependency declarations to match `providers/github/Cargo.toml`
on the current `main`. In particular, `omnifs-sdk` must point at the workspace
version and not an old path/git revision.

### Re-register the provider in the workspace

The workspace-level `Cargo.toml` on `main` dropped every non-dns/github/test
provider. Re-add `providers/huggingface` to its `members` array. Example diff:

```toml
[workspace]
members = [
    "crates/cli",
    "crates/host",
    "providers/dns",
    "providers/github",
+   "providers/huggingface",
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

Auth is optional. The huggingface API works anonymously with lower rate
limits; a bearer token unlocks higher limits and private repos. The host
injects the `Authorization: Bearer <token>` header when configured.

```rust
Capabilities {
    auth_types: vec!["bearer-token".to_string()],
    domains: vec!["huggingface.co".to_string()],
    ..Default::default()
}
```

Remove any `token` / `api_key` fields on `Config` or `State` and any
manual `Authorization` header injection in `http_ext.rs` or handler
code. The provider does not need to know whether a token is configured;
the host injects it at callout-dispatch time if present.

Rate-limit surfacing uses `ProviderError::rate_limited(...)` (see the
error-constructors section below).

Domains covered:

  - `huggingface.co`

Mount config shape (with optional auth):

```json
{
  "plugin": "huggingface.wasm",
  "mount": "/huggingface",
  "auth": [{"type": "bearer-token", "token_env": "HF_TOKEN", "domain": "huggingface.co"}]
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

---

## Reference body (original MIGRATION_PLAN.md; subordinate to the corrections above)

> The content that follows was written for the old-SDK worktree at
> `/Users/raul/W/gvfs/.worktrees/providers/huggingface/MIGRATION_PLAN.md`.
> Read it for provider-specific shape, path tables, gotchas, and per-file
> migration notes. Wherever a passage conflicts with the corrections above
> (auth handling, POST shape, error constructors, rest captures, destructive
> action for crates-io), the corrections win.

# huggingface provider migration plan

Sonnet-executable plan to port `providers/huggingface` from the pre-`6343486`
SDK to the path-first handler + callouts SDK now on `main`. All code blocks
are inline. Do not follow "see X" pointers; this document is
self-contained.

## Summary

This worktree was forked at `7742e99` and has a working but pre-redesign
provider (`api.rs`, `entities/`, `events.rs`, `http_ext.rs`, `lib.rs`,
`provider.rs`, `types.rs`, ~3000 LoC). `main`'s `6343486` replaces the
`mounts! { ... }` + `Dir`/`Subtree` trait model with free-function
handlers declared by `#[omnifs_sdk::dir]` / `#[file]` / `#[subtree]` in
`#[handlers]` impl blocks, removes `materialize()`, turns subtree handoff
into a `SubtreeRef` terminal folded into lookup/list, narrows the effect
model to request/response `Callout`s, and moves cache invalidation into
`EventOutcome` returned from `on_event`. All HTTP access goes through
`cx.http().get(url)...send_body()/send()`; no `.request()`, no HEAD, no
POST in the SDK HTTP builder today.

The migration is substantial but mechanical:

1. Merge `main` into the worktree; take `main`'s `crates/omnifs-sdk*`,
   `crates/omnifs-mount-schema`, `wit/provider.wit` wholesale.
2. Add `"providers/huggingface"` to the workspace `members` at
   `.../Cargo.toml`.
3. Rewrite `provider.rs`, `lib.rs`, each `entities/*` file as a
   `#[handlers] impl XxxHandlers` module (free functions, no traits).
4. Keep `types.rs` and the `api.rs` HTTP client verbatim. Rewrite
   `head_resolver` to use `GET` instead of `HEAD` (SDK has no HEAD) or
   drop it and rely on the tree entry's size. Delete `http_ext.rs`.
5. Rewrite `events.rs::timer_tick` to return `Result<EventOutcome>` and
   feed its invalidations through `EventOutcome::invalidate_prefix`.
   Drive the active-path snapshot through `cx.active_paths(MOUNT_ID,
   parse)` instead of `cx.active::<T>()`.
6. Verify with `cargo fmt --check`, `cargo clippy -p
   omnifs-provider-huggingface --target wasm32-wasip2 -- -D warnings`,
   `cargo test ... --no-run`, `just check-providers`.

## Current path table (verbatim from old `mounts!`)

Source: `.worktrees/providers/huggingface/providers/huggingface/src/lib.rs`.

| Template                                                                            | Kind    | Entity              |
|-------------------------------------------------------------------------------------|---------|---------------------|
| `/`                                                                                 | dir     | `Root`              |
| `/{kind}`                                                                           | dir     | `KindRoot`          |
| `/{kind}/{namespace}`                                                               | dir     | `NamespaceRoot`     |
| `/{kind}/{namespace}/{repo}`                                                        | dir     | `Repo`              |
| `/{kind}/{namespace}/{repo}/_card`                                                  | dir     | `Card`              |
| `/{kind}/{namespace}/{repo}/_files`                                                 | subtree | `RepoFiles`         |
| `/{kind}/{namespace}/{repo}/_downloads`                                             | subtree | `RepoDownloads`     |
| `/{kind}/{namespace}/{repo}/_versions`                                              | dir     | `Versions`          |
| `/{kind}/{namespace}/{repo}/_versions/branches`                                     | dir     | `Branches`          |
| `/{kind}/{namespace}/{repo}/_versions/branches/{encoded_ref}`                       | dir     | `Branch`            |
| `/{kind}/{namespace}/{repo}/_versions/branches/{encoded_ref}/_files`                | subtree | `BranchFiles`       |
| `/{kind}/{namespace}/{repo}/_versions/branches/{encoded_ref}/_downloads`            | subtree | `BranchDownloads`   |
| `/{kind}/{namespace}/{repo}/_versions/tags`                                         | dir     | `Tags`              |
| `/{kind}/{namespace}/{repo}/_versions/tags/{encoded_ref}`                           | dir     | `Tag`               |
| `/{kind}/{namespace}/{repo}/_versions/tags/{encoded_ref}/_files`                    | subtree | `TagFiles`          |
| `/{kind}/{namespace}/{repo}/_versions/tags/{encoded_ref}/_downloads`                | subtree | `TagDownloads`      |
| `/{kind}/{namespace}/{repo}/_versions/commits`                                      | dir     | `Commits`           |
| `/{kind}/{namespace}/{repo}/_versions/commits/{sha}`                                | dir     | `Commit`            |
| `/{kind}/{namespace}/{repo}/_versions/commits/{sha}/_files`                         | subtree | `CommitFiles`       |
| `/{kind}/{namespace}/{repo}/_versions/commits/{sha}/_downloads`                     | subtree | `CommitDownloads`   |

Captures (from `lib.rs`):

- `kind: crate::types::HubKind` (values: `models`, `datasets`, `spaces`)
- `namespace: crate::types::Namespace` (newtype over `String`)
- `repo: crate::types::RepoName` (newtype over `String`)
- `encoded_ref: crate::types::EncodedRef` (percent-encoded ref name)
- `sha: crate::types::CommitSha` (hex sha, len >= 7)

## Target path table

All templates and entity meanings are preserved 1:1. The entities and
their `impl Dir/Subtree` blocks are replaced by free functions in a
single `#[handlers] impl XxxHandlers` module per source file. Downloads
subtrees are no longer `Subtree` trait implementations; they were
synthetic metadata trees (not real git repos) so they migrate to `#[dir]`
handlers that project fixed metadata file names under the download path.

| Template                                                             | Attribute  | Handler module / fn                        | Notes                                                                                                                    |
|----------------------------------------------------------------------|------------|--------------------------------------------|--------------------------------------------------------------------------------------------------------------------------|
| `/`                                                                  | `#[dir]`   | `root::RootHandlers::root`                 | Static children: `models`, `datasets`, `spaces`                                                                          |
| `/{kind}`                                                            | `#[dir]`   | `root::RootHandlers::kind_root`            | Lists known namespaces. `kind: HubKind` (FromStr already exists).                                                        |
| `/{kind}/{namespace}`                                                | `#[dir]`   | `root::RootHandlers::namespace_root`       | Lists repos under a namespace                                                                                            |
| `/{kind}/{namespace}/{repo}`                                         | `#[dir]`   | `repo::RepoHandlers::repo`                 | Projects `info.json` + static children (`_card`, `_files`, `_downloads`, `_versions`)                                    |
| `/{kind}/{namespace}/{repo}/_card`                                   | `#[dir]`   | `repo::RepoHandlers::card`                 | Projects `README.md` and `metadata.json`                                                                                 |
| `/{kind}/{namespace}/{repo}/_files`                                  | `#[subtree]` | `repo::RepoHandlers::repo_files`         | `cx.git().open_repo(cache_key, clone_url).await?.tree` — default revision                                                |
| `/{kind}/{namespace}/{repo}/_downloads`                              | `#[dir]`   | `downloads::DownloadHandlers::repo_dl`     | Projects static metadata filenames (see below)                                                                           |
| `/{kind}/{namespace}/{repo}/_versions`                               | `#[dir]`   | `versions::VersionHandlers::versions`      | Static children: `branches`, `tags`, `commits`                                                                           |
| `/{kind}/{namespace}/{repo}/_versions/branches`                      | `#[dir]`   | `versions::VersionHandlers::branches`      |                                                                                                                          |
| `/{kind}/{namespace}/{repo}/_versions/branches/{encoded_ref}`        | `#[dir]`   | `versions::VersionHandlers::branch`        | Projects `ref.json`, `commit.json`                                                                                       |
| `/{kind}/{namespace}/{repo}/_versions/branches/{encoded_ref}/_files` | `#[subtree]` | `versions::VersionHandlers::branch_files` | `cx.git().open_repo(...)` with `#branch={encoded_ref}` baked into the cache key so each ref opens its own clone          |
| `/{kind}/{namespace}/{repo}/_versions/branches/{encoded_ref}/_downloads` | `#[dir]` | `downloads::DownloadHandlers::branch_dl`  |                                                                                                                          |
| `/{kind}/{namespace}/{repo}/_versions/tags`                          | `#[dir]`   | `versions::VersionHandlers::tags`          |                                                                                                                          |
| `/{kind}/{namespace}/{repo}/_versions/tags/{encoded_ref}`            | `#[dir]`   | `versions::VersionHandlers::tag`           |                                                                                                                          |
| `/{kind}/{namespace}/{repo}/_versions/tags/{encoded_ref}/_files`     | `#[subtree]` | `versions::VersionHandlers::tag_files`   |                                                                                                                          |
| `/{kind}/{namespace}/{repo}/_versions/tags/{encoded_ref}/_downloads` | `#[dir]`   | `downloads::DownloadHandlers::tag_dl`      |                                                                                                                          |
| `/{kind}/{namespace}/{repo}/_versions/commits`                       | `#[dir]`   | `versions::VersionHandlers::commits`       |                                                                                                                          |
| `/{kind}/{namespace}/{repo}/_versions/commits/{sha}`                 | `#[dir]`   | `versions::VersionHandlers::commit`        |                                                                                                                          |
| `/{kind}/{namespace}/{repo}/_versions/commits/{sha}/_files`          | `#[subtree]` | `versions::VersionHandlers::commit_files` | cache key includes sha                                                                                                   |
| `/{kind}/{namespace}/{repo}/_versions/commits/{sha}/_downloads`      | `#[dir]`   | `downloads::DownloadHandlers::commit_dl`   |                                                                                                                          |

### Capture types

The existing `types.rs` already has `FromStr` impls for `HubKind`,
`Namespace`, `RepoName`, `EncodedRef`, `CommitSha`. Those work verbatim
as handler captures. Retain `types.rs` unchanged.

### Files subtree migration (critical)

Old: `_files` subtrees were a trait impl (`Subtree for RepoFiles` etc.)
that synthesized tree listings by calling the HuggingFace tree API
(`/api/{kind}/{ns}/{repo}/tree/{revision}/...`) and inlined small files
via the resolver URL with a byte cap. This is now impossible to
replicate under the new SDK: `Subtree` is gone and subtree handoff
returns a `SubtreeRef` backed by a git clone that the host manages.

HuggingFace repos are real git repos (`https://huggingface.co/{repo}.git`
for models, `https://huggingface.co/datasets/{ns}/{repo}.git` for
datasets, `https://huggingface.co/spaces/{ns}/{repo}.git` for spaces), so
the right move is to back `_files` with `cx.git().open_repo`. The
per-revision subtrees (`branches/{encoded_ref}/_files`,
`tags/{encoded_ref}/_files`, `commits/{sha}/_files`) reuse the same
clone; the host git layer handles checkouts. Use distinct cache keys per
ref so each subtree mount gets its own bind-mounted clone.

LFS note: the `huggingface.co` git repos store large artifacts as
Git-LFS pointers in the working tree. The host does not fetch LFS blobs
today; an `_files` subtree will surface pointer files, not the real
binary content. This matches how the host already serves large content
through git. Flag as a gotcha below. Callers who need resolved
content for a specific revision can use `_downloads` (metadata) + the
`url` file to fetch via HTTP.

### Downloads subtree migration (critical)

Old: `_downloads` was a `Subtree` that re-listed the tree and injected
synthetic metadata children (`meta.json`, `url`, `etag`, `size`, etc.)
for every file leaf. This was a synthesized metadata tree, not a real
subtree. Under the new SDK we cannot implement this as a subtree (no
dynamic `list`/`read` below a subtree mount). Migrate to a dir handler
that enumerates the known metadata filenames at the leaf path.

Strategy: expose the full repo tree walk via the `_files` subtree
(backed by git). `_downloads/<path>` becomes a `#[dir]` route at the
mount's fixed metadata leaves. Because we cannot dynamically route under
a `#[dir]` for arbitrary relative paths in the new SDK, the simplest
faithful port is a thinner surface: `_downloads` exposes a single-level
projection with a `meta.json` for the repo's resolved revision and a
`files.json` listing of `DownloadMetaView` records. Each record contains
`path`, `url`, `etag`, `size`, `blob_id`, `sha256`, `xet_hash`,
`last_commit`. This preserves the substantive data while fitting the new
browse model. Sample projection for `_downloads`:

```rust
// /{kind}/{namespace}/{repo}/_downloads
let mut p = Projection::new();
p.file_with_content("files.json", json_bytes_of_download_list)?;
p.file_with_stat("README.md", FileStat::placeholder());
Ok(p)
```

If a path-faithful port of the synthetic per-file metadata tree is
required, defer that scope to a follow-up plan. The current migration
ships a flat `_downloads` projection and documents this scope change
explicitly. (Rationale: `Subtree` semantics in the new SDK are fixed to
git-backed clones; recreating the virtual metadata tree requires a new
"virtual subtree" primitive that does not exist yet.)

## SDK cheatsheet (inline, verbatim)

### Provider

```rust
// lib.rs
pub(crate) use omnifs_sdk::prelude::Result;

mod provider;
mod root;
pub(crate) mod types;

#[derive(Clone)]
pub(crate) struct State { /* runtime */ }

#[omnifs_sdk::config]
struct Config {
    #[serde(default)] hf_token: Option<String>,
    #[serde(default = "default_page_size")] page_size: u32,
}
fn default_page_size() -> u32 { 50 }
```

```rust
// provider.rs
use omnifs_sdk::prelude::*;
use crate::{Config, State};

#[provider(mounts(
    crate::root::RootHandlers,
    crate::model::ModelHandlers,
    crate::dataset::DatasetHandlers,
))]
impl HuggingFaceProvider {
    fn init(config: Config) -> Result<(State, ProviderInfo)> {
        Ok((State { /* ... */ }, ProviderInfo {
            name: "huggingface-provider".to_string(),
            version: "0.1.0".to_string(),
            description: "...".to_string(),
        }))
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["huggingface.co".to_string(), "cdn-lfs.huggingface.co".to_string()],
            auth_types: vec![],
            max_memory_mb: 64,
            needs_git: true,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 900,
        }
    }

    async fn on_event(cx: Cx<State>, event: ProviderEvent) -> Result<EventOutcome> {
        let mut outcome = EventOutcome::new();
        // Poll latest revisions on TimerTick; emit invalidate_prefix when changed.
        Ok(outcome)
    }
}
```

### Handlers (examples covering dir/file/subtree, preload, sibling files)

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
        p.dir("_models");
        p.dir("_datasets");
        p.dir("_spaces");
        Ok(p)
    }

    #[dir("/_models/{org}/{name}")]
    async fn model_dir(
        cx: &DirCx<'_, State>,
        org: String,
        name: String,
    ) -> Result<Projection> {
        let token = cx.state(|s| s.config.hf_token.clone());
        let mut req = cx.http().get(format!("https://huggingface.co/api/models/{org}/{name}"));
        if let Some(t) = token { req = req.header("Authorization", format!("Bearer {t}")); }
        let bytes = req.send_body().await?;
        let mut p = Projection::new();
        p.file_with_content("model-info.json", bytes);
        p.dir("revisions");
        Ok(p)
    }

    #[subtree("/_models/{org}/{name}/repo")]
    async fn model_repo(cx: &Cx<State>, org: String, name: String) -> Result<SubtreeRef> {
        let url = format!("https://huggingface.co/{org}/{name}.git");
        let repo = cx.git().open(url).await?;
        Ok(SubtreeRef::new(repo.tree_ref))
    }

    #[file("/_models/{org}/{name}/README.md")]
    async fn model_readme(cx: &Cx<State>, org: String, name: String) -> Result<FileContent> {
        let bytes = cx.http()
            .get(format!("https://huggingface.co/{org}/{name}/raw/main/README.md"))
            .send_body().await?;
        Ok(FileContent::bytes(bytes))
    }
}
```

Rules:

- Captures become typed args; non-`String` captures need `FromStr`.
- `DirCx<'_, S>` derefs to `Cx<S>`.
- Sync or `async fn`.
- Projection builders: `.dir(name)`, `.file(name)`,
  `.file_with_stat(name, FileStat { size })`, `.file_with_content(name,
  bytes)` (eager <= 64 KiB), `.page(PageStatus::{Exhaustive,
  More(Cursor::Opaque(..))})`, `.preload(path, bytes)` /
  `.preload_many(iter)`.
- `Lookup::with_sibling_files(iter)`,
  `FileContent::with_sibling_files(iter)` for adjacent cache fills.

### Context

- `cx.state(|s| ...)` / `cx.state_mut(|s| ...)`.
- `cx.http()`: `.get(url)`, `.header`, `.send_body().await`,
  `.send().await` (returns `HttpResponse { status, headers, body }`).
  There is no `.post`, `.request`, or `.head` in today's SDK. HEAD
  probes must be rewritten.
- `cx.git()`: `.open_repo(cache_key, clone_url).await ->
  Result<GitRepoInfo { tree }>`. Note: the task brief references
  `cx.git().open(url)` and `tree_ref`; the shipped SDK uses
  `open_repo(cache_key, clone_url)` and `GitRepoInfo.tree`. Use the
  shipped API. See `providers/github/src/repo.rs` for a live example.
- `join_all(futs)` for parallel callouts.

### Errors

`ProviderError::{not_found, invalid_input, internal, not_a_directory,
not_a_file, too_large, unimplemented}` via `omnifs_sdk::prelude::*`.

### Caching

Host owns caching. No provider LRUs/TTLs. Non-zero file sizes.
Invalidation via `EventOutcome::invalidate_path` /
`EventOutcome::invalidate_prefix` in `on_event`. Scope/identity
invalidation removed.

## Bring worktree up to main

Worktree tip `e1d0b85`, fork `7742e99`. Run from the worktree root:

```bash
cd /Users/raul/W/gvfs/.worktrees/providers/huggingface
git fetch --all --prune
git status
git merge main
```

Expected conflicts (take `main`'s versions wholesale):

- `crates/omnifs-sdk/**` -> `git checkout --theirs crates/omnifs-sdk`
- `crates/omnifs-sdk-macros/**` -> `git checkout --theirs crates/omnifs-sdk-macros`
- `crates/omnifs-mount-schema/**` (new on main, may not exist in fork)
  -> `git checkout --theirs crates/omnifs-mount-schema`
- `wit/provider.wit` -> `git checkout --theirs wit/provider.wit`
- `crates/host/**` -> `git checkout --theirs crates/host` (host was
  rebuilt around the new SDK on main)
- `crates/cli/**` -> `git checkout --theirs crates/cli`
- `providers/github/**`, `providers/dns/**`, `providers/test/**` -> take
  `main` versions; those exist on `main` as reference implementations.
- Top-level `Cargo.toml`: resolve so `members` equals `["crates/*",
  "providers/github", "providers/dns", "providers/test",
  "providers/huggingface"]` (see Cargo.toml section).
- `Cargo.lock` -> resolve to theirs, then `cargo generate-lockfile`
  at end.

Keep `providers/huggingface/**` from the current worktree (`--ours`).
That is the directory this plan rewrites.

After resolving, stage and commit the merge but do NOT attempt to build
yet. The provider code will still reference the old SDK and will fail
until the per-file migration below is complete.

```bash
git add -A
git commit -m "chore: merge main (SDK redesign) into huggingface worktree"
```

## Cargo.toml changes

### Workspace root

`/Users/raul/W/gvfs/.worktrees/providers/huggingface/Cargo.toml`: add
`"providers/huggingface"` to `members`.

```toml
[workspace]
resolver = "2"
members = ["crates/*", "providers/github", "providers/dns", "providers/test", "providers/huggingface"]
default-members = ["crates/cli", "crates/host"]
```

Keep the `[workspace.dependencies]` block from main as-is after the
merge.

### Provider crate

`/Users/raul/W/gvfs/.worktrees/providers/huggingface/providers/huggingface/Cargo.toml`:
mirror `providers/github/Cargo.toml`. Add `hashbrown` and `serde_json`.

```toml
[package]
name = "omnifs-provider-huggingface"
version = "0.1.0"
edition = "2024"
description = "omnifs provider for browsing Hugging Face repositories"
license = "MIT OR Apache-2.0"
repository = "https://github.com/raulk/omnifs"
homepage = "https://github.com/raulk/omnifs"

[lib]
crate-type = ["cdylib", "lib"]

[dependencies]
omnifs-sdk = { path = "../../crates/omnifs-sdk" }
hashbrown = "0.15"
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

## Per-file migration

Source root:
`/Users/raul/W/gvfs/.worktrees/providers/huggingface/providers/huggingface/src/`.

### `lib.rs` — REWRITE

Delete the `mounts!` block, `Root` / `KindRoot` / ... re-exports, and
the `ProviderResult` alias (use `Result` from prelude). Keep `Config`
and `State`; drop fields the new invalidation model removes.

```rust
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use hashbrown::HashMap;
pub(crate) use omnifs_sdk::prelude::Result;

mod api;
mod downloads;
mod events;
mod provider;
mod repo;
mod root;
pub(crate) mod types;
mod versions;

use crate::types::RepoKey;

#[derive(Clone)]
#[omnifs_sdk::config]
pub(crate) struct Config {
    #[serde(default)]
    pub hf_token: Option<String>,
    #[serde(default = "default_inline_read_max_bytes")]
    pub inline_read_max_bytes: u64,
    #[serde(default = "default_namespace_list_limit")]
    pub namespace_list_limit: usize,
    #[serde(default = "default_commit_history_limit")]
    pub commit_history_limit: usize,
}

fn default_inline_read_max_bytes() -> u64 { 2 * 1024 * 1024 }
fn default_namespace_list_limit() -> usize { 256 }
fn default_commit_history_limit() -> usize { 256 }

#[derive(Clone)]
pub(crate) struct State {
    pub config: Config,
    pub repo_heads: HashMap<RepoKey, String>,
    pub ref_targets: HashMap<(RepoKey, crate::types::RefKind, String), String>,
}
```

Notes:

- `seen_namespaces` is dropped. Namespace discovery was persisted in
  state to carry "known" entries across ticks. The new invalidation
  model does not need it; the namespace listing handler re-queries the
  hub each time and the host caches results until explicit invalidation.
- `ProviderError` import is no longer needed in `lib.rs`; use the
  prelude in the modules that need it.

### `provider.rs` — REWRITE

Replace the old `#[omnifs_sdk::provider]` impl and the hand-rolled
`on_event` plumbing with the new form. Model after
`providers/github/src/provider.rs`.

```rust
use omnifs_sdk::Cx;
use omnifs_sdk::prelude::*;

use crate::events::timer_tick;
use crate::{Config, State};

#[provider(mounts(
    crate::root::RootHandlers,
    crate::repo::RepoHandlers,
    crate::versions::VersionHandlers,
    crate::downloads::DownloadHandlers,
))]
impl HuggingfaceProvider {
    fn init(config: Config) -> (State, ProviderInfo) {
        (
            State {
                config,
                repo_heads: hashbrown::HashMap::new(),
                ref_targets: hashbrown::HashMap::new(),
            },
            ProviderInfo {
                name: "huggingface-provider".to_string(),
                version: "0.1.0".to_string(),
                description: "Hugging Face Hub provider for omnifs".to_string(),
            },
        )
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec![
                "huggingface.co".to_string(),
                "cdn-lfs.huggingface.co".to_string(),
            ],
            auth_types: vec!["bearer-token".to_string()],
            max_memory_mb: 128,
            needs_git: true,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 300,
        }
    }

    async fn on_event(cx: Cx<State>, event: ProviderEvent) -> Result<EventOutcome> {
        match event {
            ProviderEvent::TimerTick(_) => timer_tick(cx).await,
            _ => Ok(EventOutcome::new()),
        }
    }
}
```

### `types.rs` — KEEP VERBATIM

No SDK types are imported here. The `FromStr` impls for `HubKind`,
`Namespace`, `RepoName`, `EncodedRef`, `CommitSha` satisfy the new
handler capture requirement.

### `api.rs` — KEEP with one edit

The HTTP client functions (`load_kind_namespaces`, `load_namespace_repos`,
`load_repo_info`, `load_repo_refs`, `load_repo_commits`,
`load_commit_for_revision`, `load_tree`, `find_tree_entry`,
`read_repo_file`, `read_optional_readme`, `load_download_meta`, the
views, `json_bytes`, `join_mount`, `split_parent`, `encode_*`) are
written against `Cx<State>` and `cx.http().get(url).send()/send_body()`.
Those APIs exist in the new SDK unchanged. Keep them.

Drop `use omnifs_sdk::mount::EntryStat;` (no longer re-exported; stat is
now `FileStat`). Drop `TreeEntry::stat()`; replace callers with
`FileStat { size: NonZeroU64::new(...).unwrap_or_else(||
FileStat::placeholder().size) }`. Easier: add a helper on the handler
side and drop the EntryStat coupling from `api.rs` entirely.

Replace the one usage of `ProviderResult`:

```rust
// OLD
pub(crate) type ProviderResult<T> = std::result::Result<T, ProviderError>;
// NEW (in lib.rs): pub(crate) use omnifs_sdk::prelude::Result;
// and every `ProviderResult<T>` in api.rs -> `crate::Result<T>`.
```

Rewrite `head_resolver` to use GET (SDK has no HEAD), or delete it.
Recommendation: delete `head_resolver` and `ResolverHead`, and fill
`DownloadMetaView::etag` / `size` / `xet_hash` strictly from the tree
entry (`entry.effective_size()`, `entry.sha256()`, `entry.xet_hash`).
The download metadata is already derivable from the tree listing; the
HEAD probe was opportunistic enrichment.

If HEAD probing is genuinely needed, open a follow-up plan for adding
`Request::method(&str)` or `.head()` to the SDK HTTP builder; do not
add a provider-side workaround here.

### `http_ext.rs` — DELETE

The file is one constant (`RESOLVE_SEGMENT`). Inline that into `api.rs`
as `const RESOLVE_SEGMENT: &str = "/resolve/";` (private). Delete the
module and its `mod http_ext;` declaration in `lib.rs`.

### `entities/` — DELETE DIRECTORY, REWRITE AS FLAT HANDLER MODULES

Delete the entire `entities/` subtree (`mod.rs`, `root.rs`, `repo.rs`,
`versions.rs`, `files.rs`, `downloads.rs`). Replace with these modules
at `src/`:

#### `src/root.rs` (replaces `entities/root.rs`)

```rust
use omnifs_sdk::prelude::*;

use crate::api::{load_kind_namespaces, load_namespace_repos};
use crate::types::{HubKind, Namespace, RepoName};
use crate::{Result, State};

pub struct RootHandlers;

#[handlers]
impl RootHandlers {
    #[dir("/")]
    fn root(_cx: &DirCx<'_, State>) -> Result<Projection> {
        let mut p = Projection::new();
        p.dir("models");
        p.dir("datasets");
        p.dir("spaces");
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/{kind}")]
    async fn kind_root(cx: &DirCx<'_, State>, kind: HubKind) -> Result<Projection> {
        let namespaces = load_kind_namespaces(cx, kind).await?;
        let mut p = Projection::new();
        for ns in namespaces {
            p.dir(ns.to_string());
        }
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/{kind}/{namespace}")]
    async fn namespace_root(
        cx: &DirCx<'_, State>,
        kind: HubKind,
        namespace: Namespace,
    ) -> Result<Projection> {
        let repos: Vec<RepoName> = load_namespace_repos(cx, kind, &namespace).await?;
        let mut p = Projection::new();
        for repo in repos {
            p.dir(repo.to_string());
        }
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }
}
```

Notes:

- `seen_namespaces` warm-start logic is dropped. The host cache handles
  memoization across ticks; there is no need for the provider to keep
  its own "known" set. `load_kind_namespaces` and `load_namespace_repos`
  in `api.rs` have calls to `remember_namespace` / to the config's
  `seen_namespaces` state; delete those calls.

#### `src/repo.rs` (replaces `entities/repo.rs`)

```rust
use omnifs_sdk::prelude::*;

use crate::api::{
    json_bytes, load_repo_info, read_optional_readme, repo_info_view,
};
use crate::types::{HubKind, Namespace, RepoName};
use crate::{Result, State};

pub struct RepoHandlers;

#[handlers]
impl RepoHandlers {
    #[dir("/{kind}/{namespace}/{repo}")]
    async fn repo(
        cx: &DirCx<'_, State>,
        kind: HubKind,
        namespace: Namespace,
        repo: RepoName,
    ) -> Result<Projection> {
        let info = load_repo_info(cx, kind, &namespace, &repo, None).await?;
        let mut p = Projection::new();
        if let Ok(bytes) = json_bytes(&repo_info_view(kind, &namespace, &repo, &info)) {
            p.file_with_content("info.json", bytes);
        }
        // Static children _card, _files, _downloads, _versions are declared by
        // their sibling handlers and merged automatically by the SDK.
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/{kind}/{namespace}/{repo}/_card")]
    async fn card(
        cx: &DirCx<'_, State>,
        kind: HubKind,
        namespace: Namespace,
        repo: RepoName,
    ) -> Result<Projection> {
        let info = load_repo_info(cx, kind, &namespace, &repo, None).await?;
        let readme = read_optional_readme(cx, kind, &namespace, &repo, &info.revision()).await?;
        let mut p = Projection::new();
        if let Some(readme) = readme {
            p.file_with_content("README.md", readme);
        }
        if let Ok(bytes) = json_bytes(
            &info
                .card_data
                .clone()
                .unwrap_or_else(|| serde_json::Value::Object(Default::default())),
        ) {
            p.file_with_content("metadata.json", bytes);
        }
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[subtree("/{kind}/{namespace}/{repo}/_files")]
    async fn repo_files(
        cx: &Cx<State>,
        kind: HubKind,
        namespace: Namespace,
        repo: RepoName,
    ) -> Result<SubtreeRef> {
        let (cache_key, clone_url) = repo_git_urls(kind, &namespace, &repo, None);
        let info = cx.git().open_repo(cache_key, clone_url).await?;
        Ok(SubtreeRef::new(info.tree))
    }
}

fn repo_git_urls(
    kind: HubKind,
    namespace: &Namespace,
    repo: &RepoName,
    revision: Option<&str>,
) -> (String, String) {
    // Clone URL conventions (confirm against current HuggingFace docs before
    // shipping):
    //   models    https://huggingface.co/{ns}/{name}.git
    //   datasets  https://huggingface.co/datasets/{ns}/{name}.git
    //   spaces    https://huggingface.co/spaces/{ns}/{name}.git
    let prefix = kind.web_prefix();
    let base = if prefix.is_empty() {
        format!("{ns}/{name}", ns = namespace, name = repo)
    } else {
        format!("{prefix}/{ns}/{name}", prefix = prefix.trim_start_matches('/'), ns = namespace, name = repo)
    };
    let clone_url = format!("https://huggingface.co/{base}.git");
    let suffix = revision.map(|r| format!("#{r}")).unwrap_or_default();
    let cache_key = format!("huggingface.co/{base}{suffix}");
    (cache_key, clone_url)
}
```

#### `src/versions.rs` (replaces `entities/versions.rs`)

```rust
use omnifs_sdk::prelude::*;

use crate::api::{
    commit_view, json_bytes, load_commit_for_revision, load_repo_commits, load_repo_info,
    load_repo_refs, ref_view,
};
use crate::repo::repo_git_urls;
use crate::types::{CommitSha, EncodedRef, HubKind, Namespace, RefKind, RepoName};
use crate::{Result, State};

pub struct VersionHandlers;

#[handlers]
impl VersionHandlers {
    #[dir("/{kind}/{namespace}/{repo}/_versions")]
    fn versions(
        _cx: &DirCx<'_, State>,
        _kind: HubKind,
        _namespace: Namespace,
        _repo: RepoName,
    ) -> Result<Projection> {
        let mut p = Projection::new();
        // `branches`, `tags`, `commits` are registered as sibling dir handlers
        // and auto-merged as static children.
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/{kind}/{namespace}/{repo}/_versions/branches")]
    async fn branches(
        cx: &DirCx<'_, State>,
        kind: HubKind,
        namespace: Namespace,
        repo: RepoName,
    ) -> Result<Projection> {
        let refs = load_repo_refs(cx, kind, &namespace, &repo).await?;
        let mut p = Projection::new();
        for branch in refs.branches {
            p.dir(EncodedRef::encode(&branch.name).to_string());
        }
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/{kind}/{namespace}/{repo}/_versions/branches/{encoded_ref}")]
    async fn branch(
        cx: &DirCx<'_, State>,
        kind: HubKind,
        namespace: Namespace,
        repo: RepoName,
        encoded_ref: EncodedRef,
    ) -> Result<Projection> {
        let revision = encoded_ref
            .decode()
            .ok_or_else(|| ProviderError::invalid_input("invalid encoded branch ref"))?;
        let reference = load_repo_refs(cx, kind, &namespace, &repo)
            .await?
            .branches
            .into_iter()
            .find(|r| r.name == revision)
            .ok_or_else(|| ProviderError::not_found("branch not found"))?;
        let commit = load_commit_for_revision(
            cx,
            kind,
            &namespace,
            &repo,
            reference.target_commit.as_deref().unwrap_or(revision.as_str()),
        )
        .await?
        .unwrap_or_default();
        let mut p = Projection::new();
        if let Ok(bytes) = json_bytes(&ref_view(RefKind::Branch, &reference)) {
            p.file_with_content("ref.json", bytes);
        }
        if let Ok(bytes) = json_bytes(&commit_view(&commit)) {
            p.file_with_content("commit.json", bytes);
        }
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[subtree("/{kind}/{namespace}/{repo}/_versions/branches/{encoded_ref}/_files")]
    async fn branch_files(
        cx: &Cx<State>,
        kind: HubKind,
        namespace: Namespace,
        repo: RepoName,
        encoded_ref: EncodedRef,
    ) -> Result<SubtreeRef> {
        let revision = encoded_ref
            .decode()
            .ok_or_else(|| ProviderError::invalid_input("invalid encoded branch ref"))?;
        let (cache_key, clone_url) = repo_git_urls(kind, &namespace, &repo, Some(&revision));
        let info = cx.git().open_repo(cache_key, clone_url).await?;
        Ok(SubtreeRef::new(info.tree))
    }

    #[dir("/{kind}/{namespace}/{repo}/_versions/tags")]
    async fn tags(
        cx: &DirCx<'_, State>,
        kind: HubKind,
        namespace: Namespace,
        repo: RepoName,
    ) -> Result<Projection> {
        let refs = load_repo_refs(cx, kind, &namespace, &repo).await?;
        let mut p = Projection::new();
        for tag in refs.tags {
            p.dir(EncodedRef::encode(&tag.name).to_string());
        }
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/{kind}/{namespace}/{repo}/_versions/tags/{encoded_ref}")]
    async fn tag(
        cx: &DirCx<'_, State>,
        kind: HubKind,
        namespace: Namespace,
        repo: RepoName,
        encoded_ref: EncodedRef,
    ) -> Result<Projection> {
        let revision = encoded_ref
            .decode()
            .ok_or_else(|| ProviderError::invalid_input("invalid encoded tag ref"))?;
        let reference = load_repo_refs(cx, kind, &namespace, &repo)
            .await?
            .tags
            .into_iter()
            .find(|r| r.name == revision)
            .ok_or_else(|| ProviderError::not_found("tag not found"))?;
        let commit = load_commit_for_revision(
            cx,
            kind,
            &namespace,
            &repo,
            reference.target_commit.as_deref().unwrap_or(revision.as_str()),
        )
        .await?
        .unwrap_or_default();
        let mut p = Projection::new();
        if let Ok(bytes) = json_bytes(&ref_view(RefKind::Tag, &reference)) {
            p.file_with_content("ref.json", bytes);
        }
        if let Ok(bytes) = json_bytes(&commit_view(&commit)) {
            p.file_with_content("commit.json", bytes);
        }
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[subtree("/{kind}/{namespace}/{repo}/_versions/tags/{encoded_ref}/_files")]
    async fn tag_files(
        cx: &Cx<State>,
        kind: HubKind,
        namespace: Namespace,
        repo: RepoName,
        encoded_ref: EncodedRef,
    ) -> Result<SubtreeRef> {
        let revision = encoded_ref
            .decode()
            .ok_or_else(|| ProviderError::invalid_input("invalid encoded tag ref"))?;
        let (cache_key, clone_url) = repo_git_urls(kind, &namespace, &repo, Some(&revision));
        let info = cx.git().open_repo(cache_key, clone_url).await?;
        Ok(SubtreeRef::new(info.tree))
    }

    #[dir("/{kind}/{namespace}/{repo}/_versions/commits")]
    async fn commits(
        cx: &DirCx<'_, State>,
        kind: HubKind,
        namespace: Namespace,
        repo: RepoName,
    ) -> Result<Projection> {
        let info = load_repo_info(cx, kind, &namespace, &repo, None).await?;
        let commits = load_repo_commits(cx, kind, &namespace, &repo, &info.revision()).await?;
        let limit = cx.state(|s| s.config.commit_history_limit);
        let mut p = Projection::new();
        for commit in &commits {
            if let Some(sha) = commit.id.as_deref().and_then(CommitSha::new) {
                p.dir(sha.to_string());
            }
        }
        if commits.len() >= limit {
            p.page(PageStatus::More(Cursor::Opaque(
                "huggingface-commit-history".to_string(),
            )));
        } else {
            p.page(PageStatus::Exhaustive);
        }
        Ok(p)
    }

    #[dir("/{kind}/{namespace}/{repo}/_versions/commits/{sha}")]
    async fn commit(
        cx: &DirCx<'_, State>,
        kind: HubKind,
        namespace: Namespace,
        repo: RepoName,
        sha: CommitSha,
    ) -> Result<Projection> {
        let commit = load_commit_for_revision(cx, kind, &namespace, &repo, sha.as_ref())
            .await?
            .unwrap_or_default();
        let mut p = Projection::new();
        if let Ok(bytes) = json_bytes(&commit_view(&commit)) {
            p.file_with_content("commit.json", bytes);
        }
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[subtree("/{kind}/{namespace}/{repo}/_versions/commits/{sha}/_files")]
    async fn commit_files(
        cx: &Cx<State>,
        kind: HubKind,
        namespace: Namespace,
        repo: RepoName,
        sha: CommitSha,
    ) -> Result<SubtreeRef> {
        let (cache_key, clone_url) = repo_git_urls(kind, &namespace, &repo, Some(sha.as_ref()));
        let info = cx.git().open_repo(cache_key, clone_url).await?;
        Ok(SubtreeRef::new(info.tree))
    }
}
```

Add `pub(crate) fn repo_git_urls(...)` to `src/repo.rs` (or move it to
`src/api.rs`) and `pub(crate)` it so `versions.rs` and `downloads.rs`
can reuse it.

Note: `CommitRecord` needs `#[derive(Default)]` for
`.unwrap_or_default()` to compile. It already derives Default in
`api.rs`; keep that.

#### `src/downloads.rs` (replaces `entities/downloads.rs`)

Flat projection per the downgrade described in "Target path table".
Drops the synthetic metadata tree.

```rust
use omnifs_sdk::prelude::*;

use crate::api::{
    DownloadMetaView, json_bytes, load_download_meta, load_repo_info, load_tree,
};
use crate::types::{CommitSha, EncodedRef, HubKind, Namespace, RepoName};
use crate::{Result, State};

pub struct DownloadHandlers;

#[handlers]
impl DownloadHandlers {
    #[dir("/{kind}/{namespace}/{repo}/_downloads")]
    async fn repo_dl(
        cx: &DirCx<'_, State>,
        kind: HubKind,
        namespace: Namespace,
        repo: RepoName,
    ) -> Result<Projection> {
        let revision = load_repo_info(cx, kind, &namespace, &repo, None)
            .await?
            .revision();
        project_downloads(cx, kind, &namespace, &repo, &revision).await
    }

    #[dir("/{kind}/{namespace}/{repo}/_versions/branches/{encoded_ref}/_downloads")]
    async fn branch_dl(
        cx: &DirCx<'_, State>,
        kind: HubKind,
        namespace: Namespace,
        repo: RepoName,
        encoded_ref: EncodedRef,
    ) -> Result<Projection> {
        let revision = encoded_ref
            .decode()
            .ok_or_else(|| ProviderError::invalid_input("invalid encoded branch ref"))?;
        project_downloads(cx, kind, &namespace, &repo, &revision).await
    }

    #[dir("/{kind}/{namespace}/{repo}/_versions/tags/{encoded_ref}/_downloads")]
    async fn tag_dl(
        cx: &DirCx<'_, State>,
        kind: HubKind,
        namespace: Namespace,
        repo: RepoName,
        encoded_ref: EncodedRef,
    ) -> Result<Projection> {
        let revision = encoded_ref
            .decode()
            .ok_or_else(|| ProviderError::invalid_input("invalid encoded tag ref"))?;
        project_downloads(cx, kind, &namespace, &repo, &revision).await
    }

    #[dir("/{kind}/{namespace}/{repo}/_versions/commits/{sha}/_downloads")]
    async fn commit_dl(
        cx: &DirCx<'_, State>,
        kind: HubKind,
        namespace: Namespace,
        repo: RepoName,
        sha: CommitSha,
    ) -> Result<Projection> {
        project_downloads(cx, kind, &namespace, &repo, sha.as_ref()).await
    }
}

async fn project_downloads(
    cx: &Cx<State>,
    kind: HubKind,
    namespace: &Namespace,
    repo: &RepoName,
    revision: &str,
) -> Result<Projection> {
    let tree = load_tree(cx, kind, namespace, repo, revision, "").await?;
    let files: Vec<DownloadMetaView> = join_all(
        tree.iter()
            .filter(|e| e.is_file())
            .map(|entry| load_download_meta(cx, kind, namespace, repo, revision, entry)),
    )
    .await
    .into_iter()
    .collect::<Result<Vec<_>>>()?;

    let body = json_bytes(&files)?;
    let mut p = Projection::new();
    p.file_with_content("files.json", body);
    p.page(PageStatus::Exhaustive);
    Ok(p)
}
```

If `files.json` can exceed 64 KiB for large repos (likely for
multi-hundred-file model snapshots), switch to `p.file_with_stat(...)`
and back the read with a `#[file]` handler. Pseudocode skeleton to add
if that path is needed:

```rust
#[file("/{kind}/{namespace}/{repo}/_downloads/files.json")]
async fn repo_dl_files_json(
    cx: &Cx<State>,
    kind: HubKind,
    namespace: Namespace,
    repo: RepoName,
) -> Result<FileContent> {
    let revision = load_repo_info(cx, kind, &namespace, &repo, None).await?.revision();
    let tree = load_tree(cx, kind, &namespace, &repo, &revision, "").await?;
    let files = join_all(tree.iter().filter(|e| e.is_file()).map(|e|
        load_download_meta(cx, kind, &namespace, &repo, &revision, e)
    )).await.into_iter().collect::<Result<Vec<_>>>()?;
    Ok(FileContent::new(json_bytes(&files)?))
}
```

(Plus analogous `#[file]` handlers for each `_downloads/files.json`
mount if needed.)

### `events.rs` — REWRITE

Replace the `ProviderResponse`-returning coroutine with a plain
`async fn timer_tick(cx: Cx<State>) -> Result<EventOutcome>`. Drop
`ActivePaths::collect` and `cx.active::<T>()`; use
`cx.active_paths(MOUNT_ID, parse)` with MOUNT_ID constants generated by
`#[handlers]`. Drop explicit `cx.invalidate_prefix(..)` calls; push
invalidations into `EventOutcome::invalidate_prefix`.

```rust
use omnifs_sdk::Cx;
use omnifs_sdk::prelude::*;

use crate::api::{load_repo_info, load_repo_refs};
use crate::types::{HubKind, Namespace, RefKind, RepoKey, RepoName};
use crate::{Result, State};

// MOUNT_ID constants generated by #[handlers] are on the path structs
// named after each handler function, e.g. RepoPath, CardPath, etc.
// Use the same template string as declared on the handler attribute.
const REPO_TEMPLATE: &str = "/{kind}/{namespace}/{repo}";
const BRANCHES_TEMPLATE: &str = "/{kind}/{namespace}/{repo}/_versions/branches";
const TAGS_TEMPLATE: &str = "/{kind}/{namespace}/{repo}/_versions/tags";
const BRANCH_TEMPLATE: &str = "/{kind}/{namespace}/{repo}/_versions/branches/{encoded_ref}";
const TAG_TEMPLATE: &str = "/{kind}/{namespace}/{repo}/_versions/tags/{encoded_ref}";

fn parse_repo_path(path: &str) -> Option<RepoKey> {
    let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if segs.len() < 3 { return None; }
    let kind: HubKind = segs[0].parse().ok()?;
    let ns = Namespace::new(segs[1])?;
    let name = RepoName::new(segs[2])?;
    Some(RepoKey::new(kind, ns, name))
}

fn parse_ref_path(path: &str) -> Option<(RepoKey, RefKind, String)> {
    // /{kind}/{namespace}/{repo}/_versions/{branches|tags}/{encoded_ref}
    let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if segs.len() != 6 || segs[3] != "_versions" { return None; }
    let repo = parse_repo_path(path)?;
    let ref_kind = match segs[4] {
        "branches" => RefKind::Branch,
        "tags"     => RefKind::Tag,
        _          => return None,
    };
    let encoded = crate::types::EncodedRef::from_str(segs[5]).ok()?;
    let decoded = encoded.decode()?;
    Some((repo, ref_kind, decoded))
}

pub(crate) async fn timer_tick(cx: Cx<State>) -> Result<EventOutcome> {
    let mut outcome = EventOutcome::new();

    let mut repo_keys = cx.active_paths(REPO_TEMPLATE, parse_repo_path);
    repo_keys.sort_by(|a, b| {
        a.kind.cmp(&b.kind)
            .then(a.namespace.cmp(&b.namespace))
            .then(a.repo.cmp(&b.repo))
    });
    repo_keys.dedup();

    let mut ref_set_keys: Vec<(RepoKey, RefKind)> = Vec::new();
    for path in cx.active_paths(BRANCHES_TEMPLATE, parse_repo_path) {
        ref_set_keys.push((path, RefKind::Branch));
    }
    for path in cx.active_paths(TAGS_TEMPLATE, parse_repo_path) {
        ref_set_keys.push((path, RefKind::Tag));
    }
    ref_set_keys.sort();
    ref_set_keys.dedup();

    let mut specific_refs = cx.active_paths(BRANCH_TEMPLATE, parse_ref_path);
    specific_refs.extend(cx.active_paths(TAG_TEMPLATE, parse_ref_path));
    specific_refs.sort();
    specific_refs.dedup();

    if repo_keys.is_empty() && ref_set_keys.is_empty() && specific_refs.is_empty() {
        return Ok(outcome);
    }

    // ---- Repo heads ----
    let repo_outcomes = join_all(repo_keys.iter().cloned().map(|key| {
        let cx = cx.clone();
        async move {
            (key.clone(), load_repo_info(&cx, key.kind, &key.namespace, &key.repo, None).await)
        }
    }))
    .await;

    let mut repo_head_updates = Vec::new();
    for (key, info) in repo_outcomes {
        let Ok(info) = info else { continue };
        let head = info.revision();
        let previous = cx.state(|s| s.repo_heads.get(&key).cloned());
        if previous.as_deref().is_some_and(|p| p != head) {
            outcome.invalidate_prefix(format!(
                "/{}/{}/{}",
                key.kind, key.namespace, key.repo
            ));
        }
        repo_head_updates.push((key, head));
    }

    // ---- Refs ----
    let ref_repos: Vec<RepoKey> = {
        let mut r: std::collections::BTreeSet<RepoKey> = ref_set_keys.iter().map(|(k, _)| k.clone()).collect();
        for (k, _, _) in &specific_refs { r.insert(k.clone()); }
        r.into_iter().collect()
    };
    let ref_outcomes = join_all(ref_repos.iter().cloned().map(|key| {
        let cx = cx.clone();
        async move {
            (key.clone(), load_repo_refs(&cx, key.kind, &key.namespace, &key.repo).await)
        }
    }))
    .await;

    let mut snapshot_updates: Vec<(RepoKey, RefKind, std::collections::BTreeMap<String, String>)> = Vec::new();
    let mut specific_ref_updates: Vec<((RepoKey, RefKind, String), Option<String>)> = Vec::new();

    for (key, refs) in ref_outcomes {
        let Ok(refs) = refs else { continue };

        let branch_snapshot: std::collections::BTreeMap<String, String> = refs.branches.iter()
            .filter_map(|r| r.target_commit.as_ref().map(|t| (r.name.clone(), t.clone())))
            .collect();
        let tag_snapshot: std::collections::BTreeMap<String, String> = refs.tags.iter()
            .filter_map(|r| r.target_commit.as_ref().map(|t| (r.name.clone(), t.clone())))
            .collect();

        if ref_set_keys.contains(&(key.clone(), RefKind::Branch)) {
            let previous: std::collections::BTreeMap<String, String> = cx.state(|s| s.ref_targets.iter()
                .filter(|((k, rk, _), _)| *k == key && *rk == RefKind::Branch)
                .map(|((_, _, n), v)| (n.clone(), v.clone()))
                .collect());
            if !previous.is_empty() && previous != branch_snapshot {
                outcome.invalidate_prefix(format!(
                    "/{}/{}/{}/_versions/branches",
                    key.kind, key.namespace, key.repo
                ));
            }
            snapshot_updates.push((key.clone(), RefKind::Branch, branch_snapshot.clone()));
        }
        if ref_set_keys.contains(&(key.clone(), RefKind::Tag)) {
            let previous: std::collections::BTreeMap<String, String> = cx.state(|s| s.ref_targets.iter()
                .filter(|((k, rk, _), _)| *k == key && *rk == RefKind::Tag)
                .map(|((_, _, n), v)| (n.clone(), v.clone()))
                .collect());
            if !previous.is_empty() && previous != tag_snapshot {
                outcome.invalidate_prefix(format!(
                    "/{}/{}/{}/_versions/tags",
                    key.kind, key.namespace, key.repo
                ));
            }
            snapshot_updates.push((key.clone(), RefKind::Tag, tag_snapshot.clone()));
        }

        for (cand_key, rk, ref_name) in &specific_refs {
            if cand_key != &key { continue; }
            let current = match *rk {
                RefKind::Branch => branch_snapshot.get(ref_name).cloned(),
                RefKind::Tag    => tag_snapshot.get(ref_name).cloned(),
            };
            let previous = cx.state(|s| s.ref_targets.get(&(key.clone(), *rk, ref_name.clone())).cloned());
            if previous.is_some() && previous != current {
                let seg = match *rk { RefKind::Branch => "branches", RefKind::Tag => "tags" };
                let encoded = crate::types::EncodedRef::encode(ref_name).to_string();
                outcome.invalidate_prefix(format!(
                    "/{}/{}/{}/_versions/{seg}/{encoded}",
                    key.kind, key.namespace, key.repo
                ));
            }
            specific_ref_updates.push(((key.clone(), *rk, ref_name.clone()), current));
        }
    }

    cx.state_mut(|s| {
        for (k, head) in repo_head_updates.drain(..) {
            s.repo_heads.insert(k, head);
        }
        for (k, rk, snap) in snapshot_updates.drain(..) {
            s.ref_targets.retain(|(ck, crk, _), _| !(*ck == k && *crk == rk));
            s.ref_targets.extend(snap.into_iter().map(|(n, t)| ((k.clone(), rk, n), t)));
        }
        for ((k, rk, n), cur) in specific_ref_updates.drain(..) {
            if let Some(target) = cur {
                s.ref_targets.insert((k, rk, n), target);
            } else {
                s.ref_targets.remove(&(k, rk, n));
            }
        }
    });

    Ok(outcome)
}
```

Notes:

- `RepoKey` derives on main must include `Ord`/`PartialOrd` for
  `BTreeSet` usage. Check `types.rs`; the current file derives
  `PartialEq, Eq, Hash` but not `Ord`. Add `PartialOrd, Ord` to
  `RepoKey` as part of this migration.
- `parse_repo_path` etc. rely on the structural shape of the mount IDs.
  Prefer deriving templates from the generated path structs' MOUNT_ID
  constants if the module path lets you import them (e.g. `use
  crate::repo::RepoPath; const REPO_TEMPLATE: &str = RepoPath::MOUNT_ID;`).
  The github provider does exactly this
  (`providers/github/src/events.rs:26`).
- `DEFAULT_REFRESH_SECS` is no longer needed here; it lives in
  `capabilities()`.

## Event handling migration (summary)

| Old                                                | New                                                       |
|----------------------------------------------------|-----------------------------------------------------------|
| `cx.active::<Repo>()`                              | `cx.active_paths(RepoPath::MOUNT_ID, parse_repo_path)`    |
| `cx.invalidate_prefix(MountPrefix::new(..)).await` | `outcome.invalidate_prefix("/kind/ns/repo")`              |
| `ProviderResponse::Done(ActionResult::Ok)`         | `Ok(EventOutcome::new())` / `Ok(outcome)`                 |
| `Pin<Box<dyn Future...>>` in `on_event`            | `async fn on_event(cx: Cx<State>, event: ProviderEvent) -> Result<EventOutcome>` |
| `Effect::CacheInvalidateScope/Identity`            | REMOVED — prefix-only invalidation                        |
| `Effect::GitListTree/ReadBlob/...`                 | REMOVED — only `cx.git().open_repo(..)` is supported     |

Timer-tick polling behaviour is preserved: the handler gates work on
`cx.active_paths(..)`, diffs against state snapshots, emits prefix
invalidations for changed heads / ref sets / specific refs, and updates
state in a single `state_mut`.

## Verification checklist

Run from the worktree root
`/Users/raul/W/gvfs/.worktrees/providers/huggingface`.

```bash
# 1. Formatting
cargo fmt --check

# 2. Provider clippy on wasm target (must be clean under -D warnings)
cargo clippy -p omnifs-provider-huggingface \
  --target wasm32-wasip2 -- -D warnings

# 3. Provider tests compile on wasm target
cargo test -p omnifs-provider-huggingface \
  --target wasm32-wasip2 --no-run

# 4. Full provider matrix (clippy + test-compile on all providers)
just check-providers

# 5. Optional: host build still clean after merge
cargo check -p omnifs-host
cargo check -p omnifs-cli
```

Passing criteria:

- `cargo fmt --check` returns 0.
- `clippy -D warnings` returns 0 with no provider-side unwraps on
  `NonZeroU64::new` of zero-size placeholders.
- `cargo test --no-run` compiles every test harness.
- `just check-providers` green for dns, github, test, and huggingface.

## Risks and gotchas

1. **HEAD probing gone.** SDK HTTP builder only supports GET. The old
   `head_resolver` added `etag` / `x-linked-size` / `x-xet-hash` on
   top of the tree entry data. Delete the HEAD path; enrich
   `DownloadMetaView` strictly from the tree entry. If headers are
   required, file a follow-up to add `Request::method(&str)` to
   `omnifs-sdk::http`; do not work around it with GET-then-discard.

2. **LFS pointers in `_files` subtree.** Model / dataset / space git
   repos store weights and large artifacts as LFS pointers. The host
   git layer surfaces those pointers verbatim, not the resolved binary.
   Large-model consumers will see the pointer text, not the weights.
   Document this limitation in the provider README if present.

3. **Gated and private repos.** The `hf_token` config field is wired
   into `Config` but no handler attaches `Authorization: Bearer` yet.
   The `api.rs` GETs also need to send the token when present. Thread
   it through by reading `cx.state(|s| s.config.hf_token.clone())` at
   the top of each `load_*` and conditionally adding the header to the
   `Request`.

4. **Eager-bytes budget.** `Projection::file_with_content` has a 64 KiB
   cap (`MAX_PROJECTED_BYTES`). `metadata.json` / `README.md` /
   `info.json` can exceed this for large cards. For any file that may
   exceed 64 KiB, prefer `p.file_with_stat(...)` + a sibling `#[file]`
   handler that returns a full `FileContent::new(bytes)`.

5. **Per-revision subtree cache keys.** The git callout cache key must
   distinguish repo-default / branch / tag / sha clones. Use
   `huggingface.co/{prefix}/{ns}/{name}#{revision?}` so the host does
   not collide mounts. The clone URL stays the same
   (`https://huggingface.co/{prefix}/{ns}/{name}.git`); host git driver
   does the checkout.

6. **`CommitRecord::default()`.** The old branch/tag handlers built a
   fallback record carrying `reference.target_commit` into
   `CommitRecord.id`. The new code uses `unwrap_or_default()` which
   loses that context. If callers depend on `commit.json.id` being
   populated even on miss, reinstate the manual fallback:

   ```rust
   .unwrap_or_else(|| CommitRecord {
       id: reference.target_commit.clone(),
       ..CommitRecord::default()
   })
   ```

7. **Namespace-list warm-start dropped.** The old `seen_namespaces`
   set was additive across ticks so stale-but-known namespaces kept
   listing. The new code re-queries; empty hub responses will
   temporarily empty the listing. If that regression matters, move the
   additive set into the host cache via `preload` rather than
   reintroducing provider LRUs. Provider-side LRUs are forbidden.

8. **`MountPrefix` is gone.** Invalidations are plain strings under
   `EventOutcome::invalidate_prefix(&str)`. Paths must start with `/`
   or the leading-`/` gets stripped by `normalize_path`. Use absolute
   paths; the SDK normalizes.

9. **WIT merge conflict on subtree terminals.** The current
   `wit/provider.wit` shipped on main embeds `list-result::subtree`
   and `lookup-result::subtree`. Taking `main` wholesale on the merge
   is the right call; do not try to reconcile fragments from the old
   fork.

10. **Downloads subtree scope downgrade.** The old provider's
    `_downloads` was a full synthetic metadata tree. The flat
    `files.json` projection in this plan preserves the data but loses
    per-file `url` / `etag` / ... individual files. If a faithful
    re-port is required, that is a follow-up that needs a new SDK
    primitive (virtual subtree) and does not belong in this migration.

11. **`serde_json::Value` through the SDK.** The old code used
    `omnifs_sdk::serde_json::Value`. On main the SDK no longer
    re-exports `serde_json`, so add `serde_json = "1"` as a direct
    provider dep and import `serde_json::Value` etc. directly.

12. **`RepoKey` ordering.** Events code needs `RepoKey: Ord` for
    `BTreeSet` operations. Add `#[derive(PartialOrd, Ord)]` alongside
    the existing derives in `types.rs`.

---

## Verification

- `cargo fmt --check`
- `cargo clippy -p omnifs-provider-huggingface --target wasm32-wasip2 -- -D warnings`
- `cargo test -p omnifs-provider-huggingface --target wasm32-wasip2 --no-run`
- `just check-providers`

All must pass. If `just` is not on PATH, note that in the PR body and run
the equivalent `cargo` commands from the root of this branch's worktree.

## Commit

Conventional:

```
feat(huggingface): migrate provider to path-first handler SDK
```

Body: one paragraph naming the major structural changes and the base SDK
PRs (#28 `feat/sdk-http-post-support`, #29 `feat/sdk-path-rest-captures`,
#D `feat/sdk-error-constructors`).

## PR

- Title: `feat(huggingface): migrate provider to path-first handler SDK`
- Body: summary + link to this branch's `PLAN.md` + verification results
  (which cargo commands ran, which passed, which were skipped and why).
