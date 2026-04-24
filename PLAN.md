# feat/migrate-arxiv

The `omnifs-provider-arxiv` provider mirrors arXiv into a projected filesystem: browse papers by category, author, and search query (with pagination), and read per-paper metadata plus PDF, tarball source, and a synthesized `references.json` summary.

## Blocked by

This plan cannot start execution until all three of these have merged into `main`:

- PR #28 `feat/sdk-http-post-support` — https://github.com/raulk/omnifs/pull/28
- PR #29 `feat/sdk-path-rest-captures` — https://github.com/raulk/omnifs/pull/29
- PR TBD `feat/sdk-error-constructors` — error constructor convenience methods

## Execution model

This branch was created off `main` at `6343486`. To execute:

1. `git -C /Users/raul/W/gvfs worktree add /Users/raul/W/gvfs/.worktrees/migrate-arxiv feat/migrate-arxiv`
2. Work in that worktree only.
3. Bring in the provider source from the old worktree at
   `/Users/raul/W/gvfs/.worktrees/providers/arxiv/providers/arxiv/`
   per the "Port provider source" step below.
4. Execute this PLAN.md end-to-end. Corrections in the "Migration
   corrections" section are authoritative over anything in the reference
   body that contradicts them.
5. Run the Verification commands listed near the bottom.
6. Commit on the `feat/migrate-arxiv` branch, push, open PR.


## Port provider source

This branch is off `main` at `6343486`, so there is NO merge from
`wip/provider-arxiv-impl` and NO `git merge main`. The wip branch carries OLD-SDK infrastructure
that must not land here. Only provider-local files come over, file by file,
using `git checkout <old-branch> -- <path>` (this pulls the file contents into
the working tree and index without touching anything else).

### Files to copy verbatim (no touch-ups beyond rust import paths / `ProviderResult` → `Result`)

- `providers/arxiv/src/types.rs`
- `providers/arxiv/src/query.rs`

Bring each over with:

```bash
git checkout wip/provider-arxiv-impl -- providers/arxiv/src/types.rs
git checkout wip/provider-arxiv-impl -- providers/arxiv/src/query.rs
```

### Files to copy then touch up

- `providers/arxiv/src/api.rs` — copy over, then apply the corrections in
  the relevant reference-body sections (auth removal, POST rewrites, error
  constructor substitutions).
- `providers/arxiv/src/events.rs` — copy over, then apply the corrections in
  the relevant reference-body sections (auth removal, POST rewrites, error
  constructor substitutions).

Bring them in with:

```bash
git checkout wip/provider-arxiv-impl -- providers/arxiv/src/api.rs
git checkout wip/provider-arxiv-impl -- providers/arxiv/src/events.rs
```

Then edit in place.

### Files to create fresh (do NOT copy from the wip branch)

- `providers/arxiv/src/lib.rs`
- `providers/arxiv/src/provider.rs`
- `providers/arxiv/src/root.rs`
- `providers/arxiv/src/handlers/ (module tree covering categories, authors, queries, papers)`

### Files to DISCARD (do NOT bring to this branch)

- `providers/arxiv/src/entities/ (entire folder)`
- `providers/arxiv/src/old provider.rs`
- `providers/arxiv/src/old lib.rs`

These are old-SDK artifacts (entity projections, tree walkers, routes tables,
manual http_ext wrappers for auth). The new SDK shape replaces them with
path-first handlers.

### Bring over the provider Cargo.toml

```bash
git checkout wip/provider-arxiv-impl -- providers/arxiv/Cargo.toml
```

Then update its SDK dependency declarations to match `providers/github/Cargo.toml`
on the current `main`. In particular, `omnifs-sdk` must point at the workspace
version and not an old path/git revision.

### Re-register the provider in the workspace

The workspace-level `Cargo.toml` on `main` dropped every non-dns/github/test
provider. Re-add `providers/arxiv` to its `members` array. Example diff:

```toml
[workspace]
members = [
    "crates/cli",
    "crates/host",
    "providers/dns",
    "providers/github",
+   "providers/arxiv",
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
    domains: vec!["export.arxiv.org".to_string()],
    ..Default::default()
}
```

Do NOT carry any `token`, `api_key`, or `oauth_access_token` fields on
`Config` or `State`. Do NOT add an `Authorization` header anywhere in
handler code. If the body of the original plan shows otherwise, it is
superseded by this section.

Domains covered (for host redirect/policy purposes):

  - `export.arxiv.org`

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
> `/Users/raul/W/gvfs/.worktrees/providers/arxiv/MIGRATION_PLAN.md`.
> Read it for provider-specific shape, path tables, gotchas, and per-file
> migration notes. Wherever a passage conflicts with the corrections above
> (auth handling, POST shape, error constructors, rest captures, destructive
> action for crates-io), the corrections win.

# arxiv provider — migration plan (OLD SDK → path-first SDK)

## Summary

The `omnifs-provider-arxiv` provider mirrors arXiv into a projected
filesystem: browse papers by category, author, and search query (with
pagination), and read per-paper metadata plus PDF, tarball source, and
a synthesized `references.json` summary. It was authored against the
OLD mount-table / entity-projection SDK; this plan migrates it to the
NEW path-first / free-function handler SDK on `main` without changing
the surfaced filesystem shape or any upstream HTTP behavior.

## Starting state

- Worktree branch: `wip/provider-arxiv-impl`, tip `e1d0b85`
  (OLD SDK commits from `feat/stage2-impl`).
- Fork point from `main`: `7742e99`.
- `providers/arxiv/` is currently **untracked** in the worktree (see
  `git status`); it sits alongside OLD-SDK versions of `providers/dns`,
  `providers/github`, `crates/omnifs-sdk*`, `crates/omnifs-mount-schema`,
  and `wit/provider.wit`.
- Main has two commits ahead: `98e1a6d` (docs) and `6343486` (the big
  SDK/runtime refactor). Everything under `crates/omnifs-sdk*`,
  `crates/omnifs-mount-schema`, `providers/dns`, `providers/github`,
  `providers/test`, `wit/provider.wit`, and `Cargo.toml` changed.

## Current path table (OLD SDK, verbatim)

From `providers/arxiv/src/lib.rs`:

```rust
omnifs_sdk::mounts! {
    capture category: crate::types::CategoryKey;
    capture author: crate::types::EncodedSelector;
    capture query: crate::types::EncodedSelector;
    capture paper: crate::types::PaperKey;
    capture page: u32;
    capture version: crate::types::VersionKey;

    "/" (dir) => Root;
    "/_categories" (dir) => Categories;
    "/_categories/{category}" (dir) => CategorySelector;
    "/_categories/{category}/_latest" (dir) => CategoryLatest;
    "/_categories/{category}/_latest/{paper}" (subtree) => CategoryLatestPaper;
    "/_categories/{category}/_pages" (dir) => CategoryPages;
    "/_categories/{category}/_pages/{page}" (dir) => CategoryPage;
    "/_categories/{category}/_pages/{page}/{paper}" (subtree) => CategoryPagePaper;
    "/_authors" (dir) => Authors;
    "/_authors/{author}" (dir) => AuthorSelector;
    "/_authors/{author}/_latest" (dir) => AuthorLatest;
    "/_authors/{author}/_latest/{paper}" (subtree) => AuthorLatestPaper;
    "/_authors/{author}/_pages" (dir) => AuthorPages;
    "/_authors/{author}/_pages/{page}" (dir) => AuthorPage;
    "/_authors/{author}/_pages/{page}/{paper}" (subtree) => AuthorPagePaper;
    "/_queries" (dir) => Queries;
    "/_queries/{query}" (dir) => QuerySelector;
    "/_queries/{query}/_latest" (dir) => QueryLatest;
    "/_queries/{query}/_latest/{paper}" (subtree) => QueryLatestPaper;
    "/_queries/{query}/_pages" (dir) => QueryPages;
    "/_queries/{query}/_pages/{page}" (dir) => QueryPage;
    "/_queries/{query}/_pages/{page}/{paper}" (subtree) => QueryPagePaper;
    "/_papers" (dir) => Papers;
    "/_papers/{paper}" (dir) => Paper;
    "/_papers/{paper}/pdf.pdf" (file) => PaperPdf;
    "/_papers/{paper}/source.tar.gz" (file) => PaperSource;
    "/_papers/{paper}/references.json" (file) => PaperReferences;
    "/_papers/{paper}/_versions" (dir) => PaperVersions;
    "/_papers/{paper}/_versions/{version}" (dir) => PaperVersion;
    "/_papers/{paper}/_versions/{version}/pdf.pdf" (file) => PaperVersionPdf;
    "/_papers/{paper}/_versions/{version}/source.tar.gz" (file) => PaperVersionSource;
    "/_papers/{paper}/_versions/{version}/references.json" (file) => PaperVersionReferences;
}
```

## Target path table (NEW SDK)

Every mount becomes a free function on a `Handlers` struct with a
`#[dir(...)]`, `#[file(...)]`, or `#[subtree(...)]` attribute. Capture
names stay the same. Path templates are unchanged. All captures already
have `FromStr` impls (`CategoryKey`, `EncodedSelector`, `PaperKey`,
`VersionKey`, and built-in `u32`), so no renames are needed.

Group assignment (used in `#[provider(mounts(...))]`):

- `RootHandlers` — `/`, `/_categories`, `/_authors`, `/_queries`, `/_papers`
- `CategoryHandlers` — all `/_categories/{category}/...` paths
- `AuthorHandlers` — all `/_authors/{author}/...` paths
- `QueryHandlers` — all `/_queries/{query}/...` paths
- `PaperHandlers` — all `/_papers/{paper}/...` paths (including
  `_versions` and versioned variants)

The `*Paper` subtree terminals (e.g. `CategoryLatestPaper`) become
`#[subtree(...)]` handlers. In the OLD SDK those were custom
`Subtree` impls that walked a synthetic paper tree. In the NEW SDK
`#[subtree(...)]` expects a `SubtreeRef` (a backing `tree_ref` handle),
which is intended for git-backed handoffs. arXiv has no git tree for a
paper, so the honest translation is to **not** emit subtrees here; we
model `/.../_latest/{paper}` and `/.../_pages/{page}/{paper}` as
nested `#[dir(...)]` + `#[file(...)]` handlers that synthesize the
per-paper shape (metadata files + `pdf.pdf` + `source.tar.gz` +
`references.json` + `_versions/...`). This preserves the OLD browse
shape exactly while staying within the callout-only NEW SDK.

Concretely the selector-scoped paper paths expand to the same shape
as `/_papers/{paper}/...`:

```
/_categories/{category}/_latest/{paper}                              (dir)
/_categories/{category}/_latest/{paper}/<metadata leaf>              (file, projected eager)
/_categories/{category}/_latest/{paper}/pdf.pdf                      (file)
/_categories/{category}/_latest/{paper}/source.tar.gz                (file)
/_categories/{category}/_latest/{paper}/references.json              (file)
/_categories/{category}/_latest/{paper}/_versions                    (dir)
/_categories/{category}/_latest/{paper}/_versions/{version}          (dir)
/_categories/{category}/_latest/{paper}/_versions/{version}/pdf.pdf  (file)
/_categories/{category}/_latest/{paper}/_versions/{version}/source.tar.gz
/_categories/{category}/_latest/{paper}/_versions/{version}/references.json
```

and likewise under `_pages/{page}/{paper}`, `/_authors/{author}/...`
and `/_queries/{query}/...`. To avoid 5× boilerplate the plan
factors the per-paper surface into a shared helper function
(`paper::project_paper_dir`, `paper::read_paper_leaf`, etc.) and thin
handler wrappers instantiate the scopes.

**No path renames are required.** All captures parse via `FromStr`
in the current `types.rs`.

## SDK cheatsheet (inlined, do NOT paraphrase — follow verbatim)

### Provider registration

```rust
// lib.rs
use std::collections::BTreeMap;
pub(crate) use omnifs_sdk::prelude::Result;

mod provider;
mod root;
pub(crate) mod types;

#[derive(Clone)]
pub(crate) struct State { /* runtime state */ }

#[omnifs_sdk::config]
struct Config {
    #[serde(default = "default_page_size")]
    page_size: u32,
}

fn default_page_size() -> u32 { 50 }
```

```rust
// provider.rs
use omnifs_sdk::prelude::*;
use crate::{Config, State};

#[provider(mounts(
    crate::root::RootHandlers,
    crate::paper::PaperHandlers,
))]
impl ArxivProvider {
    fn init(config: Config) -> Result<(State, ProviderInfo)> {
        Ok((
            State { /* ... */ },
            ProviderInfo {
                name: "arxiv-provider".to_string(),
                version: "0.1.0".to_string(),
                description: "...".to_string(),
            },
        ))
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["export.arxiv.org".to_string(), "arxiv.org".to_string()],
            auth_types: vec![],
            max_memory_mb: 64,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 3600,
        }
    }

    // Optional: event handling for timer ticks and file change events
    async fn on_event(cx: Cx<State>, event: ProviderEvent) -> Result<EventOutcome> {
        let mut outcome = EventOutcome::new();
        // match event { ProviderEvent::TimerTick(ctx) => ..., _ => {} }
        outcome.invalidate_prefix("/_categories");
        Ok(outcome)
    }
}
```

### Free-function handlers

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
        p.dir("_categories");
        p.dir("_authors");
        Ok(p)
    }

    #[file("/_about.json")]
    fn about(cx: &Cx<State>) -> Result<FileContent> {
        let body = cx.state(|s| serde_json::to_vec(&s.info).unwrap());
        Ok(FileContent::bytes(body))
    }

    #[dir("/_papers/{paper}")]
    async fn paper_dir(
        cx: &DirCx<'_, State>,
        paper: crate::types::PaperKey,
    ) -> Result<Projection> {
        let meta_bytes = cx.http()
            .get(format!("https://export.arxiv.org/api/query?id_list={paper}"))
            .send_body()
            .await?;
        let mut p = Projection::new();
        p.file_with_content("metadata.json", meta_bytes.clone());
        p.file("pdf.pdf");
        p.file("source.tar.gz");
        Ok(p)
    }

    #[file("/_papers/{paper}/pdf.pdf")]
    async fn paper_pdf(cx: &Cx<State>, paper: crate::types::PaperKey) -> Result<FileContent> {
        let bytes = cx.http()
            .get(format!("https://arxiv.org/pdf/{paper}"))
            .send_body()
            .await?;
        Ok(FileContent::bytes(bytes))
    }

    #[subtree("/_repos/{owner}/{name}")]
    async fn repo_subtree(
        cx: &Cx<State>,
        owner: String,
        name: String,
    ) -> Result<SubtreeRef> {
        let url = format!("https://github.com/{owner}/{name}.git");
        let repo = cx.git().open(url).await?;
        Ok(SubtreeRef::new(repo.tree_ref))
    }
}
```

Notes:
- Path captures become typed args. Anything other than `String` must implement `FromStr` (the SDK parses via `FromStr`).
- `DirCx<'_, S>` derefs to `Cx<S>`; you can use any `Cx` method.
- Handlers may be sync or `async fn`.
- Use `Projection::new()` then `.dir(name)`, `.file(name)`, `.file_with_stat(name, stat)`, `.file_with_content(name, bytes)` (eager bytes ≤ 64 KiB).
- Pagination: `p.page(PageStatus::More(Cursor::Opaque("cursor".into())))` or `p.page(PageStatus::Exhaustive)`.
- Preload cache: `p.preload(path, bytes)` / `p.preload_many(iter)` — the host caches those paths alongside the listing.

### Context `Cx<S>`

- `cx.state(|s: &S| ...)` / `cx.state_mut(|s: &mut S| ...)` — sync read/write of state.
- `cx.http()` — HTTP builder. Methods: `.get(url)`, `.post(url)`, `.header(k,v)`, `.json(&body)`, `.send_body().await -> Result<Vec<u8>>`, `.send().await -> Result<HttpResponse>`.
- `cx.git()` — git builder. `.open(url).await -> Result<GitRepoInfo>` (has `.tree_ref: u64`).
- `join_all(futures)` — run N callouts concurrently in a single yield/resume round trip.

### Caching model

- Host owns all caching. Providers MUST NOT use their own LRUs or TTLs.
- Projected file sizes must be non-zero. `Projection::file(name)` uses a 4096-byte placeholder size; use `.file_with_stat(name, FileStat { size: NonZeroU64::new(N).unwrap() })` if you know the real size.
- Sibling/preload idioms:
  - `Projection::preload(path, bytes)` on directory listings.
  - `Lookup::with_sibling_files(iter_of_ProjectedFile)` on lookup results.
  - `FileContent::with_sibling_files(iter_of_ProjectedFile)` on file reads.
- Invalidation is host-side. Providers express invalidation only from `on_event` handlers via `EventOutcome::invalidate_path(path)` / `invalidate_prefix(prefix)`. Scope and identity invalidation are GONE.

### Errors

`use omnifs_sdk::prelude::*;` brings `Result<T>`, `ProviderError`, `ProviderErrorKind`. Construct via `ProviderError::not_found(msg)`, `::invalid_input(msg)`, `::internal(msg)`, `::not_a_directory(msg)`, `::not_a_file(msg)`, `::unimplemented(msg)`.

### Browse terminals (for subtree/lookup flows)

- `Projection` → dir listings (most common).
- `Lookup::entry(entry)` / `Lookup::file(name, bytes)` / `Lookup::dir(name)` / `Lookup::subtree(tree_ref)` / `Lookup::not_found()` — only used internally by the SDK; handlers return `Projection`/`FileContent`/`SubtreeRef` and the SDK maps them.
- `FileContent::bytes(bytes)` for file reads (current runtime only supports eager bytes; streaming/ranged variants are reserved but not wired).

## Bring worktree up to main

**Recommended approach: `git merge main` with a known conflict set.**

The worktree has only 2 commits on `main` ahead of its fork point
(`98e1a6d docs`, `6343486 refactor!`), and the big refactor deletes/
rewrites the whole OLD SDK in-tree. Merging will flag conflicts
precisely where the OLD branch has its own rewrite of the same files.
Rebasing is not useful because the OLD branch has a dozen commits
that each invented their own version of the same SDK; cherry-picking
is not useful because `6343486` is the target end-state.

Step-by-step:

```bash
cd /Users/raul/W/gvfs/.worktrees/providers/arxiv

# 1) Save the arxiv source outside the worktree so the merge cannot
#    touch it. Everything we want to preserve is currently UNTRACKED.
mkdir -p /tmp/omnifs-arxiv-backup
cp -a providers/arxiv /tmp/omnifs-arxiv-backup/arxiv-src
cp -a docs/arxiv-provider-design.md /tmp/omnifs-arxiv-backup/ 2>/dev/null || true

# 2) Ensure the working tree is clean w.r.t. the tracked set. The
#    untracked providers/arxiv/ directory is fine to leave in place;
#    it will not conflict with main because main has no providers/arxiv.
git status

# 3) Merge main. Expect conflicts in crates/omnifs-sdk*,
#    crates/omnifs-mount-schema, providers/dns, providers/github,
#    providers/test, wit/provider.wit, Cargo.toml, and compose files.
git merge main
```

**Conflict resolution policy: take `main` for every conflict.** The
OLD-SDK versions of those paths are entirely superseded. Concretely:

```bash
# For every conflicted file, pick the main (incoming) version:
git checkout --theirs \
  crates/omnifs-sdk \
  crates/omnifs-sdk-macros \
  crates/omnifs-mount-schema \
  crates/cli \
  crates/host \
  providers/dns \
  providers/github \
  providers/test \
  wit/provider.wit \
  Cargo.toml \
  Cargo.lock \
  compose.yaml \
  compose.ci.yaml \
  justfile \
  rust-toolchain.toml \
  rustfmt.toml \
  Dockerfile \
  README.md \
  CLAUDE.md

# Then re-stage
git add -A

# Then resolve anything left by hand. After the automated resolution
# there should be no conflicts in providers/arxiv/ because providers/
# arxiv/ is not present on main.

git commit --no-edit
```

**Fallback 1: rebase is not recommended** — it would replay 12+ OLD-SDK
commits, each triggering the same conflict; use merge.

**Fallback 2: if merge is too noisy**, manually copy the critical
subtrees from the main checkout:

```bash
# From a clean checkout of main at /Users/raul/W/gvfs:
rm -rf \
  crates/omnifs-sdk crates/omnifs-sdk-macros crates/omnifs-mount-schema \
  crates/cli crates/host \
  providers/dns providers/github providers/test \
  wit/provider.wit

cp -a /Users/raul/W/gvfs/crates/omnifs-sdk        crates/
cp -a /Users/raul/W/gvfs/crates/omnifs-sdk-macros crates/
cp -a /Users/raul/W/gvfs/crates/omnifs-mount-schema crates/
cp -a /Users/raul/W/gvfs/crates/cli               crates/
cp -a /Users/raul/W/gvfs/crates/host              crates/
cp -a /Users/raul/W/gvfs/providers/dns            providers/
cp -a /Users/raul/W/gvfs/providers/github         providers/
cp -a /Users/raul/W/gvfs/providers/test           providers/
cp    /Users/raul/W/gvfs/wit/provider.wit         wit/provider.wit
cp    /Users/raul/W/gvfs/Cargo.toml               Cargo.toml
cp    /Users/raul/W/gvfs/Cargo.lock               Cargo.lock
```

Do not modify `providers/arxiv/` during this step — it must remain
intact (source material for the per-file rewrites below).

After either approach, verify:

```bash
cargo metadata --format-version=1 --no-deps >/dev/null
cargo check -p omnifs-sdk
cargo check -p omnifs-provider-dns --target wasm32-wasip2
cargo check -p omnifs-provider-github --target wasm32-wasip2
```

The arxiv provider will not compile until the rewrites below are done;
the other provider sanity-checks confirm the SDK is in place.

## Per-file migration

The existing layout:

```
providers/arxiv/
  Cargo.toml
  src/
    lib.rs
    api.rs
    events.rs
    provider.rs
    query.rs
    types.rs
    entities/
      mod.rs
      paper.rs
      root.rs
      selectors.rs
```

Target layout:

```
providers/arxiv/
  Cargo.toml
  src/
    lib.rs
    provider.rs
    api.rs
    query.rs
    types.rs
    paper.rs        # per-paper projection helpers (shared by all scopes)
    root.rs         # handlers for /, /_categories, /_authors, /_queries, /_papers
    categories.rs   # handlers for /_categories/{category}/...
    authors.rs      # handlers for /_authors/{author}/...
    queries.rs      # handlers for /_queries/{query}/...
    papers.rs       # handlers for /_papers/{paper}/...
```

The OLD `entities/` directory is deleted. `events.rs` is deleted (the
single constant it exposes is unused; `refresh_interval_secs` lives in
`capabilities()`). All `mounts!` and entity `impl` code is removed; the
HTTP, parse, URL, type, and selector-spec code is kept intact.

### File: `src/lib.rs` — REWRITE (replace entirely)

```rust
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

//! arxiv-provider: arXiv virtual filesystem provider for omnifs.

pub(crate) use omnifs_sdk::prelude::Result;

mod api;
mod authors;
mod categories;
mod paper;
mod papers;
mod provider;
mod queries;
mod query;
mod root;
pub(crate) mod types;

#[derive(Clone)]
#[omnifs_sdk::config]
pub struct Config {
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    #[serde(default = "default_selector_refresh_secs")]
    pub selector_refresh_secs: u32,
    #[serde(default)]
    pub allow_reference_extraction: bool,
}

fn default_page_size() -> u32 {
    50
}

fn default_selector_refresh_secs() -> u32 {
    3600
}

#[derive(Clone)]
pub struct State {
    pub config: Config,
}
```

Notes:
- `pub(crate) type ProviderResult<T>` is GONE. Every `ProviderResult<T>`
  in the kept files becomes `Result<T>` imported from
  `omnifs_sdk::prelude` (already re-exported above).
- `allow_reference_extraction` stays in `Config` for now (no behavior
  change: current code never implements reference extraction, it only
  returns the `"status": "unavailable"` stub). Leaving it in place
  preserves forward compatibility for a later enhancement; it costs
  nothing today.

### File: `src/provider.rs` — REWRITE (replace entirely)

```rust
use omnifs_sdk::prelude::*;

use crate::{Config, State};

#[provider(mounts(
    crate::root::RootHandlers,
    crate::categories::CategoryHandlers,
    crate::authors::AuthorHandlers,
    crate::queries::QueryHandlers,
    crate::papers::PaperHandlers,
))]
impl ArxivProvider {
    fn init(config: Config) -> Result<(State, ProviderInfo)> {
        Ok((
            State { config },
            ProviderInfo {
                name: "arxiv-provider".to_string(),
                version: "0.1.0".to_string(),
                description: "arXiv provider for omnifs".to_string(),
            },
        ))
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec![
                "export.arxiv.org".to_string(),
                "arxiv.org".to_string(),
            ],
            auth_types: vec![],
            max_memory_mb: 64,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 3600,
        }
    }
}
```

### File: `src/events.rs` — DELETE

The only export is `pub(crate) const DEFAULT_REFRESH_SECS: u32 = 3600;`
and nothing references it. `refresh_interval_secs` is supplied directly
in `capabilities()` above.

### File: `src/api.rs` — KEEP, with a trivial result-alias fix

Replace the type alias with the prelude import. Specifically, change
the top imports:

```rust
// BEFORE (top of file):
use omnifs_sdk::{Cx, prelude::*};
...
use crate::{ProviderResult, State};

// AFTER:
use omnifs_sdk::prelude::*;
...
use crate::State;
```

Then, in the signatures of `fetch_selector_page`, `fetch_checked_selector_page`,
`fetch_paper_detail`, `download_pdf`, `download_source`, `fetch_text`,
`fetch_bytes`, `parse_feed`, `parse_entry`, replace every occurrence of
`ProviderResult<T>` with `Result<T>`. Body content is unchanged.

Note: the file already uses `cx.http().get(..).header(..).send_body().await`,
which is the NEW SDK's HTTP builder shape — it did not change between
the OLD and NEW SDK. No further edits.

### File: `src/query.rs` — KEEP, with a trivial result-alias fix

Identical treatment to `api.rs`:

```rust
// BEFORE:
use omnifs_sdk::prelude::ProviderError;
...
use crate::ProviderResult;

// AFTER:
use omnifs_sdk::prelude::*;
```

Replace every `ProviderResult<T>` return type with `Result<T>`. Do NOT
touch `ProviderError::not_found(...)` call sites — they are unchanged.

### File: `src/types.rs` — KEEP VERBATIM

The file uses only `std`, `serde`, and `core::str::FromStr`. It
compiles against either SDK with no edits. The `FromStr` impls on
`CategoryKey`, `EncodedSelector`, `PaperKey`, `VersionKey` are exactly
what the NEW SDK's handler macro wants for typed captures.

### File: `src/entities/mod.rs` — DELETE

### File: `src/entities/root.rs` — DELETE (logic moves to `src/root.rs`)

### File: `src/entities/paper.rs` — DELETE (logic moves to `src/paper.rs`
and `src/papers.rs`)

### File: `src/entities/selectors.rs` — DELETE (logic moves to
`src/categories.rs`, `src/authors.rs`, `src/queries.rs`, and the
shared helpers in `src/paper.rs`)

### New file: `src/paper.rs` — CREATE

Shared helpers for the per-paper surface. Callable from any scope
(`_papers/{paper}`, `/_categories/.../{paper}`, etc.). This is a pure
refactor of the OLD `entities/paper.rs` body into scope-agnostic
functions. No behavior changes.

```rust
//! Per-paper projection and read helpers, shared by all scopes
//! (`/_papers/{paper}`, `/_categories/.../{paper}`, etc.).

use std::num::NonZeroU64;

use omnifs_sdk::prelude::*;
use serde_json::{Map, Value, json};

use crate::api::{download_pdf, download_source, fetch_paper_detail};
use crate::query::{decode_paper_key, paper_abs_url, paper_pdf_url, paper_source_url};
use crate::types::{PaperDetail, PaperKey, SelectorContext, VersionKey};
use crate::{Result, State};

/// The fixed set of text-metadata leaf names emitted at the paper root
/// and at each `_versions/vN` directory. The host caches these through
/// `Projection::file_with_content`, so a later stat/read hits cache.
pub(crate) const METADATA_LEAF_NAMES: &[&str] = &[
    "arxiv_id.txt",
    "title.txt",
    "abstract.txt",
    "authors.txt",
    "categories.txt",
    "published.txt",
    "updated.txt",
    "version.txt",
    "doi.txt",
    "journal_ref.txt",
    "abs.url",
    "metadata.json",
];

/// Build a paper directory projection (the `{paper}` level). Emits
/// eager metadata files, file placeholders for pdf/source/references,
/// and the `_versions` subdirectory.
pub(crate) async fn project_paper_dir(
    cx: &Cx<State>,
    paper: &PaperKey,
    selector_context: Option<SelectorContext>,
) -> Result<Projection> {
    let detail = load_paper_detail(cx, paper, selector_context).await?;
    let mut p = Projection::new();
    for (name, bytes) in metadata_files(&detail, None) {
        p.file_with_content(name, bytes);
    }
    p.file("pdf.pdf");
    p.file("source.tar.gz");
    p.file_with_content("references.json", references_json_bytes(&detail, None));
    p.dir("_versions");
    p.page(PageStatus::Exhaustive);
    Ok(p)
}

/// Build the `_versions` directory projection: one child dir per
/// known version `vN`.
pub(crate) async fn project_versions_dir(
    cx: &Cx<State>,
    paper: &PaperKey,
    selector_context: Option<SelectorContext>,
) -> Result<Projection> {
    let detail = load_paper_detail(cx, paper, selector_context).await?;
    let mut p = Projection::new();
    for version in 1..=detail.latest_version {
        p.dir(format!("v{version}"));
    }
    p.page(PageStatus::Exhaustive);
    Ok(p)
}

/// Build the `_versions/vN` directory projection.
pub(crate) async fn project_version_dir(
    cx: &Cx<State>,
    paper: &PaperKey,
    version: &VersionKey,
    selector_context: Option<SelectorContext>,
) -> Result<Projection> {
    let detail = load_paper_detail(cx, paper, selector_context).await?;
    let version = validate_version_key(&detail, version)?;
    let mut p = Projection::new();
    for (name, bytes) in metadata_files(&detail, Some(version)) {
        p.file_with_content(name, bytes);
    }
    p.file("pdf.pdf");
    p.file("source.tar.gz");
    p.file_with_content(
        "references.json",
        references_json_bytes(&detail, Some(version)),
    );
    p.page(PageStatus::Exhaustive);
    Ok(p)
}

/// Fetch the pdf bytes for a paper at (optionally) a specific version.
pub(crate) async fn read_paper_pdf(
    cx: &Cx<State>,
    paper: &PaperKey,
    version: Option<u32>,
) -> Result<FileContent> {
    let raw_id = base_raw_id(paper)?;
    let bytes = download_pdf(cx, &raw_id, version).await?;
    Ok(FileContent::bytes(bytes))
}

/// Fetch the tarball source for a paper at (optionally) a specific version.
pub(crate) async fn read_paper_source(
    cx: &Cx<State>,
    paper: &PaperKey,
    version: Option<u32>,
) -> Result<FileContent> {
    let raw_id = base_raw_id(paper)?;
    let bytes = download_source(cx, &raw_id, version).await?;
    Ok(FileContent::bytes(bytes))
}

/// Build the synthesized references.json bytes for the given paper at
/// the given version.
pub(crate) async fn read_paper_references(
    cx: &Cx<State>,
    paper: &PaperKey,
    version: Option<u32>,
    selector_context: Option<SelectorContext>,
) -> Result<FileContent> {
    let detail = load_paper_detail(cx, paper, selector_context).await?;
    if let Some(v) = version {
        validate_version_number(&detail, v)?;
    }
    Ok(FileContent::bytes(references_json_bytes(&detail, version)))
}

// ---- helpers (unchanged logic, moved from entities/paper.rs) ----

async fn load_paper_detail(
    cx: &Cx<State>,
    paper: &PaperKey,
    selector_context: Option<SelectorContext>,
) -> Result<PaperDetail> {
    let raw_id = base_raw_id(paper)?;
    fetch_paper_detail(cx, &raw_id, selector_context).await
}

fn base_raw_id(paper: &PaperKey) -> Result<String> {
    let decoded = decode_paper_key(paper)?;
    let (base, explicit_version) = split_versioned_id(&decoded);
    if explicit_version.is_some() {
        return Err(ProviderError::not_found(
            "versioned paper ids must be accessed through _versions",
        ));
    }
    Ok(base)
}

fn split_versioned_id(raw_id: &str) -> (String, Option<u32>) {
    let bytes = raw_id.as_bytes();
    let mut split = bytes.len();
    while split > 0 && bytes[split - 1].is_ascii_digit() {
        split -= 1;
    }
    if split == bytes.len() || split == 0 || bytes[split - 1] != b'v' {
        return (raw_id.to_string(), None);
    }
    match raw_id[split..].parse::<u32>() {
        Ok(version) => (raw_id[..split - 1].to_string(), Some(version)),
        Err(_) => (raw_id.to_string(), None),
    }
}

fn validate_version_key(detail: &PaperDetail, version: &VersionKey) -> Result<u32> {
    let Some(version) = version.number() else {
        return Err(ProviderError::not_found("invalid paper version"));
    };
    validate_version_number(detail, version)?;
    Ok(version)
}

fn validate_version_number(detail: &PaperDetail, version: u32) -> Result<()> {
    if version == 0 || version > detail.latest_version {
        return Err(ProviderError::not_found("paper version not found"));
    }
    Ok(())
}

fn metadata_files(detail: &PaperDetail, version: Option<u32>) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("arxiv_id.txt", line_bytes(&detail.raw_id)),
        ("title.txt", line_bytes(&detail.title)),
        ("abstract.txt", line_bytes(&detail.abstract_text)),
        ("authors.txt", lines_bytes(&detail.authors)),
        ("categories.txt", lines_bytes(&detail.categories)),
        ("published.txt", line_bytes(&detail.published)),
        ("updated.txt", line_bytes(&detail.updated)),
        (
            "version.txt",
            line_bytes(&format!("v{}", current_version(detail, version))),
        ),
        ("doi.txt", optional_line_bytes(detail.doi.as_deref())),
        (
            "journal_ref.txt",
            optional_line_bytes(detail.journal_ref.as_deref()),
        ),
        ("abs.url", line_bytes(&paper_abs_url(&detail.raw_id, version))),
        ("metadata.json", metadata_json_bytes(detail, version)),
    ]
}

fn metadata_json_bytes(detail: &PaperDetail, version: Option<u32>) -> Vec<u8> {
    let payload = json!({
        "raw_arxiv_id": &detail.raw_id,
        "current_version": format!("v{}", current_version(detail, version)),
        "published": &detail.published,
        "updated": &detail.updated,
        "title": &detail.title,
        "abstract": &detail.abstract_text,
        "authors": &detail.authors,
        "primary_category": &detail.primary_category,
        "categories": &detail.categories,
        "doi": &detail.doi,
        "journal_ref": &detail.journal_ref,
        "abstract_url": paper_abs_url(&detail.raw_id, version),
        "pdf_url": paper_pdf_url(&detail.raw_id, version),
        "source_url": paper_source_url(&detail.raw_id, version),
        "selector_context": &detail.selector_context,
    });
    match serde_json::to_vec_pretty(&payload) {
        Ok(mut bytes) => {
            bytes.push(b'\n');
            bytes
        },
        Err(error) => {
            format!("{{\"error\":\"failed to render metadata: {error}\"}}\n").into_bytes()
        },
    }
}

fn references_json_bytes(detail: &PaperDetail, version: Option<u32>) -> Vec<u8> {
    let mut external_links = Map::new();
    external_links.insert(
        "abstract".to_string(),
        Value::String(paper_abs_url(&detail.raw_id, version)),
    );
    external_links.insert(
        "pdf".to_string(),
        Value::String(paper_pdf_url(&detail.raw_id, version)),
    );
    external_links.insert(
        "source".to_string(),
        Value::String(paper_source_url(&detail.raw_id, version)),
    );
    if let Some(doi) = &detail.doi {
        let doi_url = if doi.starts_with("http://") || doi.starts_with("https://") {
            doi.clone()
        } else {
            format!("https://doi.org/{doi}")
        };
        external_links.insert("doi".to_string(), Value::String(doi_url));
    }
    let payload = json!({
        "status": "unavailable",
        "provenance": "abs_links_only",
        "items": [],
        "external_links": external_links,
    });
    match serde_json::to_vec_pretty(&payload) {
        Ok(mut bytes) => {
            bytes.push(b'\n');
            bytes
        },
        Err(error) => {
            format!("{{\"error\":\"failed to render references: {error}\"}}\n").into_bytes()
        },
    }
}

fn current_version(detail: &PaperDetail, version: Option<u32>) -> u32 {
    version.unwrap_or(detail.latest_version)
}

fn line_bytes(value: &str) -> Vec<u8> {
    let mut bytes = value.as_bytes().to_vec();
    bytes.push(b'\n');
    bytes
}

fn optional_line_bytes(value: Option<&str>) -> Vec<u8> {
    match value {
        Some(value) if !value.is_empty() => line_bytes(value),
        _ => Vec::new(),
    }
}

fn lines_bytes(values: &[String]) -> Vec<u8> {
    if values.is_empty() {
        return Vec::new();
    }
    let mut bytes = values.join("\n").into_bytes();
    bytes.push(b'\n');
    bytes
}

// Keep the NonZeroU64 import live in case a future patch needs to emit
// an explicit FileStat on a placeholder-file projection. It's cheap.
#[allow(dead_code)]
fn non_zero_size(len: usize) -> Option<NonZeroU64> {
    u64::try_from(len).ok().and_then(NonZeroU64::new)
}
```

### New file: `src/root.rs` — CREATE

```rust
use omnifs_sdk::prelude::*;

use crate::{Result, State};

pub struct RootHandlers;

#[handlers]
impl RootHandlers {
    #[dir("/")]
    fn root(_cx: &DirCx<'_, State>) -> Result<Projection> {
        let mut p = Projection::new();
        p.dir("_categories");
        p.dir("_authors");
        p.dir("_queries");
        p.dir("_papers");
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/_categories")]
    fn categories(_cx: &DirCx<'_, State>) -> Result<Projection> {
        // Not enumerable: arXiv has no "list all categories" endpoint
        // the provider could back this with. Users navigate by name.
        let mut p = Projection::new();
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/_authors")]
    fn authors(_cx: &DirCx<'_, State>) -> Result<Projection> {
        let mut p = Projection::new();
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/_queries")]
    fn queries(_cx: &DirCx<'_, State>) -> Result<Projection> {
        let mut p = Projection::new();
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/_papers")]
    fn papers(_cx: &DirCx<'_, State>) -> Result<Projection> {
        let mut p = Projection::new();
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }
}
```

### New file: `src/categories.rs` — CREATE

```rust
use omnifs_sdk::prelude::*;

use crate::api::{fetch_checked_selector_page, fetch_selector_page};
use crate::paper::{
    project_paper_dir, project_version_dir, project_versions_dir, read_paper_pdf,
    read_paper_references, read_paper_source,
};
use crate::query::{category_spec, page_count, page_directory_name, selector_page_url};
use crate::types::{CategoryKey, PaperKey, SelectorContext, SelectorPageData, SelectorSpec, VersionKey};
use crate::{Result, State};

pub struct CategoryHandlers;

#[handlers]
impl CategoryHandlers {
    // ----------- selector root: /_categories/{category} -----------

    #[dir("/_categories/{category}")]
    async fn category_selector(
        cx: &DirCx<'_, State>,
        category: CategoryKey,
    ) -> Result<Projection> {
        let spec = category_spec(&category);
        let head = fetch_selector_page(cx, &spec, 0).await?;
        let page_size = cx.state(|s| s.config.page_size);
        let mut p = Projection::new();
        p.file_with_content("_spec.txt", selector_spec_bytes(&head, page_size));
        p.file_with_content("_feed.atom", head.feed_xml.clone());
        p.dir("_latest");
        p.dir("_pages");
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    // ----------- _latest: paper listing at page 0 -----------

    #[dir("/_categories/{category}/_latest")]
    async fn category_latest(
        cx: &DirCx<'_, State>,
        category: CategoryKey,
    ) -> Result<Projection> {
        let spec = category_spec(&category);
        let page = fetch_selector_page(cx, &spec, 0).await?;
        Ok(paper_listing_projection(&page))
    }

    #[dir("/_categories/{category}/_latest/{paper}")]
    async fn category_latest_paper(
        cx: &DirCx<'_, State>,
        category: CategoryKey,
        paper: PaperKey,
    ) -> Result<Projection> {
        let ctx = latest_context(&category, cx);
        project_paper_dir(cx, &paper, Some(ctx)).await
    }

    #[file("/_categories/{category}/_latest/{paper}/pdf.pdf")]
    async fn category_latest_paper_pdf(
        cx: &Cx<State>,
        _category: CategoryKey,
        paper: PaperKey,
    ) -> Result<FileContent> {
        read_paper_pdf(cx, &paper, None).await
    }

    #[file("/_categories/{category}/_latest/{paper}/source.tar.gz")]
    async fn category_latest_paper_source(
        cx: &Cx<State>,
        _category: CategoryKey,
        paper: PaperKey,
    ) -> Result<FileContent> {
        read_paper_source(cx, &paper, None).await
    }

    #[file("/_categories/{category}/_latest/{paper}/references.json")]
    async fn category_latest_paper_refs(
        cx: &Cx<State>,
        category: CategoryKey,
        paper: PaperKey,
    ) -> Result<FileContent> {
        let ctx = latest_context(&category, cx);
        read_paper_references(cx, &paper, None, Some(ctx)).await
    }

    #[dir("/_categories/{category}/_latest/{paper}/_versions")]
    async fn category_latest_paper_versions(
        cx: &DirCx<'_, State>,
        category: CategoryKey,
        paper: PaperKey,
    ) -> Result<Projection> {
        let ctx = latest_context(&category, cx);
        project_versions_dir(cx, &paper, Some(ctx)).await
    }

    #[dir("/_categories/{category}/_latest/{paper}/_versions/{version}")]
    async fn category_latest_paper_version(
        cx: &DirCx<'_, State>,
        category: CategoryKey,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<Projection> {
        let ctx = latest_context(&category, cx);
        project_version_dir(cx, &paper, &version, Some(ctx)).await
    }

    #[file("/_categories/{category}/_latest/{paper}/_versions/{version}/pdf.pdf")]
    async fn category_latest_paper_version_pdf(
        cx: &Cx<State>,
        _category: CategoryKey,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<FileContent> {
        let v = version
            .number()
            .ok_or_else(|| ProviderError::not_found("invalid paper version"))?;
        read_paper_pdf(cx, &paper, Some(v)).await
    }

    #[file("/_categories/{category}/_latest/{paper}/_versions/{version}/source.tar.gz")]
    async fn category_latest_paper_version_source(
        cx: &Cx<State>,
        _category: CategoryKey,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<FileContent> {
        let v = version
            .number()
            .ok_or_else(|| ProviderError::not_found("invalid paper version"))?;
        read_paper_source(cx, &paper, Some(v)).await
    }

    #[file("/_categories/{category}/_latest/{paper}/_versions/{version}/references.json")]
    async fn category_latest_paper_version_refs(
        cx: &Cx<State>,
        category: CategoryKey,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<FileContent> {
        let v = version
            .number()
            .ok_or_else(|| ProviderError::not_found("invalid paper version"))?;
        let ctx = latest_context(&category, cx);
        read_paper_references(cx, &paper, Some(v), Some(ctx)).await
    }

    // ----------- _pages: one directory per page index -----------

    #[dir("/_categories/{category}/_pages")]
    async fn category_pages(
        cx: &DirCx<'_, State>,
        category: CategoryKey,
    ) -> Result<Projection> {
        let spec = category_spec(&category);
        let head = fetch_selector_page(cx, &spec, 0).await?;
        let page_size = cx.state(|s| s.config.page_size);
        let total_pages = page_count(head.total_results, page_size);
        Ok(pages_dir_projection(total_pages))
    }

    #[dir("/_categories/{category}/_pages/{page}")]
    async fn category_page(
        cx: &DirCx<'_, State>,
        category: CategoryKey,
        page: u32,
    ) -> Result<Projection> {
        let spec = category_spec(&category);
        let page_data = fetch_checked_selector_page(cx, &spec, page).await?;
        Ok(paper_listing_projection(&page_data))
    }

    #[dir("/_categories/{category}/_pages/{page}/{paper}")]
    async fn category_page_paper(
        cx: &DirCx<'_, State>,
        category: CategoryKey,
        page: u32,
        paper: PaperKey,
    ) -> Result<Projection> {
        let ctx = page_context(&category, page, cx);
        project_paper_dir(cx, &paper, Some(ctx)).await
    }

    #[file("/_categories/{category}/_pages/{page}/{paper}/pdf.pdf")]
    async fn category_page_paper_pdf(
        cx: &Cx<State>,
        _category: CategoryKey,
        _page: u32,
        paper: PaperKey,
    ) -> Result<FileContent> {
        read_paper_pdf(cx, &paper, None).await
    }

    #[file("/_categories/{category}/_pages/{page}/{paper}/source.tar.gz")]
    async fn category_page_paper_source(
        cx: &Cx<State>,
        _category: CategoryKey,
        _page: u32,
        paper: PaperKey,
    ) -> Result<FileContent> {
        read_paper_source(cx, &paper, None).await
    }

    #[file("/_categories/{category}/_pages/{page}/{paper}/references.json")]
    async fn category_page_paper_refs(
        cx: &Cx<State>,
        category: CategoryKey,
        page: u32,
        paper: PaperKey,
    ) -> Result<FileContent> {
        let ctx = page_context(&category, page, cx);
        read_paper_references(cx, &paper, None, Some(ctx)).await
    }

    #[dir("/_categories/{category}/_pages/{page}/{paper}/_versions")]
    async fn category_page_paper_versions(
        cx: &DirCx<'_, State>,
        category: CategoryKey,
        page: u32,
        paper: PaperKey,
    ) -> Result<Projection> {
        let ctx = page_context(&category, page, cx);
        project_versions_dir(cx, &paper, Some(ctx)).await
    }

    #[dir("/_categories/{category}/_pages/{page}/{paper}/_versions/{version}")]
    async fn category_page_paper_version(
        cx: &DirCx<'_, State>,
        category: CategoryKey,
        page: u32,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<Projection> {
        let ctx = page_context(&category, page, cx);
        project_version_dir(cx, &paper, &version, Some(ctx)).await
    }

    #[file("/_categories/{category}/_pages/{page}/{paper}/_versions/{version}/pdf.pdf")]
    async fn category_page_paper_version_pdf(
        cx: &Cx<State>,
        _category: CategoryKey,
        _page: u32,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<FileContent> {
        let v = version
            .number()
            .ok_or_else(|| ProviderError::not_found("invalid paper version"))?;
        read_paper_pdf(cx, &paper, Some(v)).await
    }

    #[file("/_categories/{category}/_pages/{page}/{paper}/_versions/{version}/source.tar.gz")]
    async fn category_page_paper_version_source(
        cx: &Cx<State>,
        _category: CategoryKey,
        _page: u32,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<FileContent> {
        let v = version
            .number()
            .ok_or_else(|| ProviderError::not_found("invalid paper version"))?;
        read_paper_source(cx, &paper, Some(v)).await
    }

    #[file("/_categories/{category}/_pages/{page}/{paper}/_versions/{version}/references.json")]
    async fn category_page_paper_version_refs(
        cx: &Cx<State>,
        category: CategoryKey,
        page: u32,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<FileContent> {
        let v = version
            .number()
            .ok_or_else(|| ProviderError::not_found("invalid paper version"))?;
        let ctx = page_context(&category, page, cx);
        read_paper_references(cx, &paper, Some(v), Some(ctx)).await
    }
}

// ---- shared helpers for all selector-scoped handler modules ----

pub(crate) fn paper_listing_projection(data: &SelectorPageData) -> Projection {
    let mut p = Projection::new();
    for paper in &data.papers {
        p.dir(paper.encoded_key.clone());
    }
    p.page(PageStatus::Exhaustive);
    p
}

pub(crate) fn pages_dir_projection(total_pages: u32) -> Projection {
    let mut p = Projection::new();
    for page in 0..total_pages {
        p.dir(page_directory_name(page));
    }
    p.page(PageStatus::Exhaustive);
    p
}

pub(crate) fn selector_spec_bytes(page: &SelectorPageData, page_size: u32) -> Vec<u8> {
    format!(
        "selector_type: {}\nselector_key: {}\nselector_value: {}\nupstream_query: {}\npage_size: {}\nsort_by: {}\nsort_order: {}\nrequest_url: {}\n",
        page.spec.kind.as_str(),
        page.spec.encoded_key,
        page.spec.decoded_value,
        page.spec.upstream_query,
        page_size,
        crate::query::DEFAULT_SORT,
        crate::query::DEFAULT_SORT_ORDER,
        page.request_url,
    )
    .into_bytes()
}

fn latest_context(category: &CategoryKey, cx: &Cx<State>) -> SelectorContext {
    selector_context(&category_spec(category), 0, cx)
}

fn page_context(category: &CategoryKey, page: u32, cx: &Cx<State>) -> SelectorContext {
    selector_context(&category_spec(category), page, cx)
}

fn selector_context(spec: &SelectorSpec, page: u32, cx: &Cx<State>) -> SelectorContext {
    let page_size = cx.state(|s| s.config.page_size);
    let request_url = selector_page_url(spec, page, page_size);
    spec.selector_context(page, request_url)
}
```

### New file: `src/authors.rs` — CREATE

Structurally identical to `categories.rs`, substituting `author_spec`
for `category_spec` and `EncodedSelector` for `CategoryKey`. The
`author_spec` helper already returns `Result<SelectorSpec>`, so the
selector-building sites use `?`.

```rust
use omnifs_sdk::prelude::*;

use crate::api::{fetch_checked_selector_page, fetch_selector_page};
use crate::categories::{paper_listing_projection, pages_dir_projection, selector_spec_bytes};
use crate::paper::{
    project_paper_dir, project_version_dir, project_versions_dir, read_paper_pdf,
    read_paper_references, read_paper_source,
};
use crate::query::{author_spec, page_count, selector_page_url};
use crate::types::{EncodedSelector, PaperKey, SelectorContext, SelectorSpec, VersionKey};
use crate::{Result, State};

pub struct AuthorHandlers;

#[handlers]
impl AuthorHandlers {
    #[dir("/_authors/{author}")]
    async fn author_selector(
        cx: &DirCx<'_, State>,
        author: EncodedSelector,
    ) -> Result<Projection> {
        let spec = author_spec(&author)?;
        let head = fetch_selector_page(cx, &spec, 0).await?;
        let page_size = cx.state(|s| s.config.page_size);
        let mut p = Projection::new();
        p.file_with_content("_spec.txt", selector_spec_bytes(&head, page_size));
        p.file_with_content("_feed.atom", head.feed_xml.clone());
        p.dir("_latest");
        p.dir("_pages");
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/_authors/{author}/_latest")]
    async fn author_latest(
        cx: &DirCx<'_, State>,
        author: EncodedSelector,
    ) -> Result<Projection> {
        let spec = author_spec(&author)?;
        let page = fetch_selector_page(cx, &spec, 0).await?;
        Ok(paper_listing_projection(&page))
    }

    #[dir("/_authors/{author}/_latest/{paper}")]
    async fn author_latest_paper(
        cx: &DirCx<'_, State>,
        author: EncodedSelector,
        paper: PaperKey,
    ) -> Result<Projection> {
        let ctx = latest_context(&author, cx)?;
        project_paper_dir(cx, &paper, Some(ctx)).await
    }

    #[file("/_authors/{author}/_latest/{paper}/pdf.pdf")]
    async fn author_latest_paper_pdf(
        cx: &Cx<State>,
        _author: EncodedSelector,
        paper: PaperKey,
    ) -> Result<FileContent> {
        read_paper_pdf(cx, &paper, None).await
    }

    #[file("/_authors/{author}/_latest/{paper}/source.tar.gz")]
    async fn author_latest_paper_source(
        cx: &Cx<State>,
        _author: EncodedSelector,
        paper: PaperKey,
    ) -> Result<FileContent> {
        read_paper_source(cx, &paper, None).await
    }

    #[file("/_authors/{author}/_latest/{paper}/references.json")]
    async fn author_latest_paper_refs(
        cx: &Cx<State>,
        author: EncodedSelector,
        paper: PaperKey,
    ) -> Result<FileContent> {
        let ctx = latest_context(&author, cx)?;
        read_paper_references(cx, &paper, None, Some(ctx)).await
    }

    #[dir("/_authors/{author}/_latest/{paper}/_versions")]
    async fn author_latest_paper_versions(
        cx: &DirCx<'_, State>,
        author: EncodedSelector,
        paper: PaperKey,
    ) -> Result<Projection> {
        let ctx = latest_context(&author, cx)?;
        project_versions_dir(cx, &paper, Some(ctx)).await
    }

    #[dir("/_authors/{author}/_latest/{paper}/_versions/{version}")]
    async fn author_latest_paper_version(
        cx: &DirCx<'_, State>,
        author: EncodedSelector,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<Projection> {
        let ctx = latest_context(&author, cx)?;
        project_version_dir(cx, &paper, &version, Some(ctx)).await
    }

    #[file("/_authors/{author}/_latest/{paper}/_versions/{version}/pdf.pdf")]
    async fn author_latest_paper_version_pdf(
        cx: &Cx<State>,
        _author: EncodedSelector,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<FileContent> {
        let v = version
            .number()
            .ok_or_else(|| ProviderError::not_found("invalid paper version"))?;
        read_paper_pdf(cx, &paper, Some(v)).await
    }

    #[file("/_authors/{author}/_latest/{paper}/_versions/{version}/source.tar.gz")]
    async fn author_latest_paper_version_source(
        cx: &Cx<State>,
        _author: EncodedSelector,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<FileContent> {
        let v = version
            .number()
            .ok_or_else(|| ProviderError::not_found("invalid paper version"))?;
        read_paper_source(cx, &paper, Some(v)).await
    }

    #[file("/_authors/{author}/_latest/{paper}/_versions/{version}/references.json")]
    async fn author_latest_paper_version_refs(
        cx: &Cx<State>,
        author: EncodedSelector,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<FileContent> {
        let v = version
            .number()
            .ok_or_else(|| ProviderError::not_found("invalid paper version"))?;
        let ctx = latest_context(&author, cx)?;
        read_paper_references(cx, &paper, Some(v), Some(ctx)).await
    }

    #[dir("/_authors/{author}/_pages")]
    async fn author_pages(
        cx: &DirCx<'_, State>,
        author: EncodedSelector,
    ) -> Result<Projection> {
        let spec = author_spec(&author)?;
        let head = fetch_selector_page(cx, &spec, 0).await?;
        let page_size = cx.state(|s| s.config.page_size);
        let total_pages = page_count(head.total_results, page_size);
        Ok(pages_dir_projection(total_pages))
    }

    #[dir("/_authors/{author}/_pages/{page}")]
    async fn author_page(
        cx: &DirCx<'_, State>,
        author: EncodedSelector,
        page: u32,
    ) -> Result<Projection> {
        let spec = author_spec(&author)?;
        let page_data = fetch_checked_selector_page(cx, &spec, page).await?;
        Ok(paper_listing_projection(&page_data))
    }

    #[dir("/_authors/{author}/_pages/{page}/{paper}")]
    async fn author_page_paper(
        cx: &DirCx<'_, State>,
        author: EncodedSelector,
        page: u32,
        paper: PaperKey,
    ) -> Result<Projection> {
        let ctx = page_context(&author, page, cx)?;
        project_paper_dir(cx, &paper, Some(ctx)).await
    }

    #[file("/_authors/{author}/_pages/{page}/{paper}/pdf.pdf")]
    async fn author_page_paper_pdf(
        cx: &Cx<State>,
        _author: EncodedSelector,
        _page: u32,
        paper: PaperKey,
    ) -> Result<FileContent> {
        read_paper_pdf(cx, &paper, None).await
    }

    #[file("/_authors/{author}/_pages/{page}/{paper}/source.tar.gz")]
    async fn author_page_paper_source(
        cx: &Cx<State>,
        _author: EncodedSelector,
        _page: u32,
        paper: PaperKey,
    ) -> Result<FileContent> {
        read_paper_source(cx, &paper, None).await
    }

    #[file("/_authors/{author}/_pages/{page}/{paper}/references.json")]
    async fn author_page_paper_refs(
        cx: &Cx<State>,
        author: EncodedSelector,
        page: u32,
        paper: PaperKey,
    ) -> Result<FileContent> {
        let ctx = page_context(&author, page, cx)?;
        read_paper_references(cx, &paper, None, Some(ctx)).await
    }

    #[dir("/_authors/{author}/_pages/{page}/{paper}/_versions")]
    async fn author_page_paper_versions(
        cx: &DirCx<'_, State>,
        author: EncodedSelector,
        page: u32,
        paper: PaperKey,
    ) -> Result<Projection> {
        let ctx = page_context(&author, page, cx)?;
        project_versions_dir(cx, &paper, Some(ctx)).await
    }

    #[dir("/_authors/{author}/_pages/{page}/{paper}/_versions/{version}")]
    async fn author_page_paper_version(
        cx: &DirCx<'_, State>,
        author: EncodedSelector,
        page: u32,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<Projection> {
        let ctx = page_context(&author, page, cx)?;
        project_version_dir(cx, &paper, &version, Some(ctx)).await
    }

    #[file("/_authors/{author}/_pages/{page}/{paper}/_versions/{version}/pdf.pdf")]
    async fn author_page_paper_version_pdf(
        cx: &Cx<State>,
        _author: EncodedSelector,
        _page: u32,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<FileContent> {
        let v = version
            .number()
            .ok_or_else(|| ProviderError::not_found("invalid paper version"))?;
        read_paper_pdf(cx, &paper, Some(v)).await
    }

    #[file("/_authors/{author}/_pages/{page}/{paper}/_versions/{version}/source.tar.gz")]
    async fn author_page_paper_version_source(
        cx: &Cx<State>,
        _author: EncodedSelector,
        _page: u32,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<FileContent> {
        let v = version
            .number()
            .ok_or_else(|| ProviderError::not_found("invalid paper version"))?;
        read_paper_source(cx, &paper, Some(v)).await
    }

    #[file("/_authors/{author}/_pages/{page}/{paper}/_versions/{version}/references.json")]
    async fn author_page_paper_version_refs(
        cx: &Cx<State>,
        author: EncodedSelector,
        page: u32,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<FileContent> {
        let v = version
            .number()
            .ok_or_else(|| ProviderError::not_found("invalid paper version"))?;
        let ctx = page_context(&author, page, cx)?;
        read_paper_references(cx, &paper, Some(v), Some(ctx)).await
    }
}

fn latest_context(author: &EncodedSelector, cx: &Cx<State>) -> Result<SelectorContext> {
    selector_context(&author_spec(author)?, 0, cx)
}

fn page_context(
    author: &EncodedSelector,
    page: u32,
    cx: &Cx<State>,
) -> Result<SelectorContext> {
    selector_context(&author_spec(author)?, page, cx)
}

fn selector_context(spec: &SelectorSpec, page: u32, cx: &Cx<State>) -> Result<SelectorContext> {
    let page_size = cx.state(|s| s.config.page_size);
    let request_url = selector_page_url(spec, page, page_size);
    Ok(spec.selector_context(page, request_url))
}
```

### New file: `src/queries.rs` — CREATE

Structurally identical to `authors.rs`, substituting `query_spec`
for `author_spec`. Copy `authors.rs` verbatim and apply these textual
replacements across the whole file:

- `/_authors/{author}` → `/_queries/{query}`
- `{author}` → `{query}` in every path template
- `AuthorHandlers` → `QueryHandlers`
- `author_spec` → `query_spec`
- every function prefix `author_` → `query_`
- every function parameter named `author: EncodedSelector` stays as
  `query: EncodedSelector` (and `_author` → `_query`)
- module-local helper names `latest_context`/`page_context`/
  `selector_context` are unchanged (they are private)

No other changes.

### New file: `src/papers.rs` — CREATE

```rust
use omnifs_sdk::prelude::*;

use crate::paper::{
    project_paper_dir, project_version_dir, project_versions_dir, read_paper_pdf,
    read_paper_references, read_paper_source,
};
use crate::types::{PaperKey, VersionKey};
use crate::{Result, State};

pub struct PaperHandlers;

#[handlers]
impl PaperHandlers {
    #[dir("/_papers/{paper}")]
    async fn paper(cx: &DirCx<'_, State>, paper: PaperKey) -> Result<Projection> {
        project_paper_dir(cx, &paper, None).await
    }

    #[file("/_papers/{paper}/pdf.pdf")]
    async fn paper_pdf(cx: &Cx<State>, paper: PaperKey) -> Result<FileContent> {
        read_paper_pdf(cx, &paper, None).await
    }

    #[file("/_papers/{paper}/source.tar.gz")]
    async fn paper_source(cx: &Cx<State>, paper: PaperKey) -> Result<FileContent> {
        read_paper_source(cx, &paper, None).await
    }

    #[file("/_papers/{paper}/references.json")]
    async fn paper_refs(cx: &Cx<State>, paper: PaperKey) -> Result<FileContent> {
        read_paper_references(cx, &paper, None, None).await
    }

    #[dir("/_papers/{paper}/_versions")]
    async fn paper_versions(
        cx: &DirCx<'_, State>,
        paper: PaperKey,
    ) -> Result<Projection> {
        project_versions_dir(cx, &paper, None).await
    }

    #[dir("/_papers/{paper}/_versions/{version}")]
    async fn paper_version(
        cx: &DirCx<'_, State>,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<Projection> {
        project_version_dir(cx, &paper, &version, None).await
    }

    #[file("/_papers/{paper}/_versions/{version}/pdf.pdf")]
    async fn paper_version_pdf(
        cx: &Cx<State>,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<FileContent> {
        let v = version
            .number()
            .ok_or_else(|| ProviderError::not_found("invalid paper version"))?;
        read_paper_pdf(cx, &paper, Some(v)).await
    }

    #[file("/_papers/{paper}/_versions/{version}/source.tar.gz")]
    async fn paper_version_source(
        cx: &Cx<State>,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<FileContent> {
        let v = version
            .number()
            .ok_or_else(|| ProviderError::not_found("invalid paper version"))?;
        read_paper_source(cx, &paper, Some(v)).await
    }

    #[file("/_papers/{paper}/_versions/{version}/references.json")]
    async fn paper_version_refs(
        cx: &Cx<State>,
        paper: PaperKey,
        version: VersionKey,
    ) -> Result<FileContent> {
        let v = version
            .number()
            .ok_or_else(|| ProviderError::not_found("invalid paper version"))?;
        read_paper_references(cx, &paper, Some(v), None).await
    }
}
```

## Event handling migration

The OLD provider **did not emit any cache-invalidation effects** and
did not register any event hooks. `events.rs` was a placeholder
containing a single unused constant. Therefore:

- Do NOT add `on_event` to the NEW `#[provider(...)]` impl unless a
  specific invalidation policy is being introduced. `capabilities()`
  already sets `refresh_interval_secs: 3600`, which the host uses as a
  TimerTick cadence; without an `on_event` handler the tick is a no-op
  and nothing is invalidated.

If a future enhancement wants to periodically invalidate the selector
listings (e.g. `/_categories/**` and `/_authors/**` so the first page
refreshes), the on_event body below is the closest NEW-SDK equivalent.
Do NOT add this now; included only as a reference for a deliberate
future change.

```rust
// Reference only — not part of this migration.
async fn on_event(_cx: Cx<State>, event: ProviderEvent) -> Result<EventOutcome> {
    let mut outcome = EventOutcome::new();
    if let ProviderEvent::TimerTick(_) = event {
        outcome.invalidate_prefix("/_categories");
        outcome.invalidate_prefix("/_authors");
        outcome.invalidate_prefix("/_queries");
    }
    Ok(outcome)
}
```

## Cargo.toml changes

### `providers/arxiv/Cargo.toml`

The current file is correct as-is for the NEW SDK with one caveat: do
NOT delete the `[package.metadata.component]` block per CLAUDE.md.
Post-migration it should look like this (identical to the current OLD
file; retained verbatim for clarity):

```toml
[package]
name = "omnifs-provider-arxiv"
version = "0.1.0"
edition = "2024"
description = "omnifs provider for browsing arXiv papers"
license = "MIT OR Apache-2.0"
repository = "https://github.com/raulk/omnifs"
homepage = "https://github.com/raulk/omnifs"

[lib]
crate-type = ["cdylib", "lib"]

[dependencies]
omnifs-sdk = { path = "../../crates/omnifs-sdk" }
regex = "1"
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

No dependency changes required. `regex`, `serde`, `serde_json` all
still in use; the OLD provider did not pull `hashbrown` and the NEW
paper helpers continue to use `Vec`/`BTreeMap`-shaped APIs, so no new
dep.

### Workspace root `Cargo.toml`

After the merge the workspace root will match `main`'s:

```toml
[workspace]
resolver = "2"
members = ["crates/*", "providers/github", "providers/dns", "providers/test"]
default-members = ["crates/cli", "crates/host"]
```

`providers/arxiv` is **missing** from `members`. Add it:

```toml
[workspace]
resolver = "2"
members = [
    "crates/*",
    "providers/github",
    "providers/dns",
    "providers/test",
    "providers/arxiv",
]
default-members = ["crates/cli", "crates/host"]
```

Do not touch `default-members`; host crates stay the native defaults.

## Behavioral changes (OLD → NEW)

Items below are semantics that do NOT map one-to-one. Each is an
explicit call-out, not a hidden deviation.

1. **Subtree handoff removed.** The OLD provider expressed each
   selector-scoped paper reference as a `Subtree` that re-routed into
   a synthesized in-memory tree. The NEW SDK's `#[subtree]` is strictly
   for `tree_ref` handles (currently git only). The per-paper content
   is therefore projected by nested `#[dir]`/`#[file]` handlers that
   call the same `project_paper_dir`/`read_paper_*` helpers. The
   surfaced paths and bytes are unchanged.

2. **Scope and identity cache invalidation removed.** The OLD SDK had
   `IdentityKey` ("arxiv.paper", "arxiv.paper.version") with
   identity-based invalidation. The NEW SDK only supports
   path-prefix invalidation from `EventOutcome`. There is no
   translation today (the provider did not invoke either mechanism).
   If future work wants to invalidate all references to a given paper
   id across scopes, the closest equivalent is to call
   `outcome.invalidate_prefix(path)` for each known active-path
   prefix discovered via `cx.active_paths(mount_id, parse_fn)` in an
   `on_event(TimerTick)` handler.

3. **`Projection` no longer carries a typed `Path` parameter.** The
   NEW `Projection` is owned and scope-agnostic. Child names are
   pushed as strings. Since path captures are already typed in the
   handler signature and the parent path is implicit, this is a pure
   simplification: helpers take no `Self::Path` and emit plain
   `p.dir(name)` / `p.file_with_content(name, bytes)` calls.

4. **`FileStat::placeholder()` is the default.** In the OLD SDK file
   leaves without explicit sizes were special-cased; in the NEW SDK
   `Projection::file(name)` already uses `FileStat::placeholder()`
   (size 4096 nonzero), and `file_with_content` computes the exact
   size from the bytes. The per-paper helpers in `src/paper.rs` use
   `file_with_content` for every metadata leaf (eager bytes, always
   <64 KiB; the JSON documents are bounded by feed size).

5. **`pdf.pdf` and `source.tar.gz` are NOT preloaded.** In both OLD
   and NEW SDKs these files are too large to eagerly project; they
   remain file placeholders resolved by dedicated `#[file]` handlers
   on demand, exactly as before.

## Verification checklist

Run from the worktree root `/Users/raul/W/gvfs/.worktrees/providers/arxiv`
after the merge and all rewrites are in place. Run the commands in
this exact order:

```bash
cargo fmt --check
cargo clippy -p omnifs-sdk -- -D warnings
cargo clippy -p omnifs-provider-dns --target wasm32-wasip2 -- -D warnings
cargo clippy -p omnifs-provider-github --target wasm32-wasip2 -- -D warnings
cargo clippy -p omnifs-provider-arxiv --target wasm32-wasip2 -- -D warnings
cargo test   -p omnifs-provider-arxiv --target wasm32-wasip2 --no-run
just check-providers
```

Notes:
- `cargo test ... --no-run` is mandatory: WASM tests compile but cannot
  execute in the cargo test harness.
- `just check-providers` runs the same target-specific clippy+test
  compile across every provider; it is the gating check locally.

If any of the provider clippy runs fails under `-D warnings`, do NOT
widen the `[lints.clippy]` allow list without cause; the acceptable
allows in `providers/arxiv/Cargo.toml` are already the union of the
dns/github provider allow lists.

## Risks and gotchas specific to arxiv

1. **`EncodedSelector` fromstr accepts `%` and other encodings.** The
   `is_encoded_segment` predicate in `types.rs` allows `% : + @ ~` in
   addition to alphanumerics; this is deliberate (the selector value
   is percent-encoded and may contain colons for `"au:\"..."`-style
   constructions). The NEW SDK's `PathPattern` still uses `FromStr`
   per-capture, so the predicate continues to protect path traversal
   without any additional rules. Do not regex-validate the capture at
   the handler level; rely on `EncodedSelector::from_str`.

2. **`PaperKey` vs versioned ids.** `base_raw_id` rejects any
   `PaperKey` whose decoded id ends in `vN`, with the message
   "versioned paper ids must be accessed through _versions". This is
   kept; the new handlers call the same helper. Do not relax it, or
   a `/_papers/1234.5678v2` URL would silently read v2 and hide the
   `_versions` surface.

3. **`allow_reference_extraction` config flag.** Present in OLD
   `Config`, wired to nothing; `references.json` always returns the
   `"status": "unavailable"` stub. Keep the flag in `Config` for
   forward compatibility (as in the OLD code) even though no handler
   branches on it.

4. **arXiv XML decoding is hand-rolled.** `src/api.rs` uses a small
   regex + hand-rolled entity decoder for `&amp;`/`&lt;`/`&gt;`/`&quot;`
   /`&apos;` plus `&#NNN;` and `&#xHH;`. Do not swap it for a full
   XML parser during this migration: the rewrite scope is SDK
   plumbing, not parsing. Any such upgrade is a separate change.

5. **Pagination: `Projection::page(PageStatus::Exhaustive)`.** The
   arxiv feed returns a page-bounded slice; every selector page
   handler sets `PageStatus::Exhaustive` because the children set at
   that path *is* exhaustive for that page (the pagination boundary
   is modeled at the `_pages/N` dir level, not as cursor-paged
   children). Do not emit `PageStatus::More(_)` from any of these
   handlers — it would tell the host to keep asking for more children
   of a page that has none.

6. **Eager metadata is <64 KiB, but the Atom feed is not.** The OLD
   `CategorySelector` (and its author/query twins) exposed an
   `_feed.atom` file with the raw feed XML. We continue to eagerly
   project it via `file_with_content`. arXiv's Atom responses for a
   `max_results=50` page are comfortably under 64 KiB, so the
   `MAX_PROJECTED_BYTES = 64 * 1024` cap in `Projection` is not a
   concern at default `page_size`. If a mount config raises
   `page_size` past ~150, the feed may exceed the cap and
   `Projection::file_with_content` will record an error. In that
   case, change `_feed.atom` to a plain `#[file]` handler that
   returns `FileContent::bytes(head.feed_xml)` lazily on read. This
   is a config-driven risk, not a default-config one.

7. **No `providers/test` updates required.** The test provider lives
   in `providers/test` and is only used by host unit tests; no arxiv
   code touches it.

8. **Do not add `needs_streaming: true`.** Both `pdf.pdf` and
   `source.tar.gz` can be multi-megabyte. The NEW SDK documents
   stream/range variants as "reserved but not wired through the
   current host runtime" (`handler.rs` read path errors with
   `ProviderError::unimplemented` if a handler returns anything other
   than `FileContent::Bytes`). Continue to use `FileContent::bytes(..)`.
   Large-file streaming is a separate future enhancement tracked at
   the SDK level.

---

## Verification

- `cargo fmt --check`
- `cargo clippy -p omnifs-provider-arxiv --target wasm32-wasip2 -- -D warnings`
- `cargo test -p omnifs-provider-arxiv --target wasm32-wasip2 --no-run`
- `just check-providers`

All must pass. If `just` is not on PATH, note that in the PR body and run
the equivalent `cargo` commands from the root of this branch's worktree.

## Commit

Conventional:

```
feat(arxiv): migrate provider to path-first handler SDK
```

Body: one paragraph naming the major structural changes and the base SDK
PRs (#28 `feat/sdk-http-post-support`, #29 `feat/sdk-path-rest-captures`,
#D `feat/sdk-error-constructors`).

## PR

- Title: `feat(arxiv): migrate provider to path-first handler SDK`
- Body: summary + link to this branch's `PLAN.md` + verification results
  (which cargo commands ran, which passed, which were skipped and why).
