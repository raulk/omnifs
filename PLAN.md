# feat/migrate-gmail

- Worktree: `/Users/raul/W/gvfs/.worktrees/providers/gmail` - Worktree tip: `e1d0b85` on branch `wip/provider-gmail-impl` - Fork point with `main`: `7742e99` - Main tip: `6343486` - The worktree was branched before the big provider SDK redesign landed on main (`refactor!: redesign provider SDK and host runtime around path-first handlers and callouts`).

## Blocked by

This plan cannot start execution until all three of these have merged into `main`:

- PR #28 `feat/sdk-http-post-support` — https://github.com/raulk/omnifs/pull/28
- PR #29 `feat/sdk-path-rest-captures` — https://github.com/raulk/omnifs/pull/29
- PR TBD `feat/sdk-error-constructors` — error constructor convenience methods

## Execution model

This branch was created off `main` at `6343486`. To execute:

1. `git -C /Users/raul/W/gvfs worktree add /Users/raul/W/gvfs/.worktrees/migrate-gmail feat/migrate-gmail`
2. Work in that worktree only.
3. Bring in the provider source from the old worktree at
   `/Users/raul/W/gvfs/.worktrees/providers/gmail/providers/gmail/`
   per the "Port provider source" step below.
4. Execute this PLAN.md end-to-end. Corrections in the "Migration
   corrections" section are authoritative over anything in the reference
   body that contradicts them.
5. Run the Verification commands listed near the bottom.
6. Commit on the `feat/migrate-gmail` branch, push, open PR.


## Port provider source

This branch is off `main` at `6343486`, so there is NO merge from
`wip/provider-gmail-impl` and NO `git merge main`. The wip branch carries OLD-SDK infrastructure
that must not land here. Only provider-local files come over, file by file,
using `git checkout <old-branch> -- <path>` (this pulls the file contents into
the working tree and index without touching anything else).

### Files to copy verbatim (no touch-ups beyond rust import paths / `ProviderResult` → `Result`)

- `providers/gmail/src/types.rs`

Bring each over with:

```bash
git checkout wip/provider-gmail-impl -- providers/gmail/src/types.rs
```

### Files to copy then touch up

- `providers/gmail/src/api.rs` — copy over, then apply the corrections in
  the relevant reference-body sections (auth removal, POST rewrites, error
  constructor substitutions).

Bring them in with:

```bash
git checkout wip/provider-gmail-impl -- providers/gmail/src/api.rs
```

Then edit in place.

### Files to create fresh (do NOT copy from the wip branch)

- `providers/gmail/src/lib.rs`
- `providers/gmail/src/provider.rs`
- `providers/gmail/src/root.rs`
- `providers/gmail/src/handlers/ (labels, threads, messages, attachments)`

### Files to DISCARD (do NOT bring to this branch)

- `providers/gmail/src/http_ext.rs`
- `providers/gmail/src/routes.rs`
- `providers/gmail/src/old provider.rs`
- `providers/gmail/src/old lib.rs`

These are old-SDK artifacts (entity projections, tree walkers, routes tables,
manual http_ext wrappers for auth). The new SDK shape replaces them with
path-first handlers.

### Bring over the provider Cargo.toml

```bash
git checkout wip/provider-gmail-impl -- providers/gmail/Cargo.toml
```

Then update its SDK dependency declarations to match `providers/github/Cargo.toml`
on the current `main`. In particular, `omnifs-sdk` must point at the workspace
version and not an old path/git revision.

### Re-register the provider in the workspace

The workspace-level `Cargo.toml` on `main` dropped every non-dns/github/test
provider. Re-add `providers/gmail` to its `members` array. Example diff:

```toml
[workspace]
members = [
    "crates/cli",
    "crates/host",
    "providers/dns",
    "providers/github",
+   "providers/gmail",
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
    domains: vec!["gmail.googleapis.com".to_string(), "www.googleapis.com".to_string()],
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

  - `gmail.googleapis.com`
  - `www.googleapis.com`

Mount config shape the user supplies:

```json
{
  "plugin": "gmail.wasm",
  "mount": "/gmail",
  "auth": [{"type": "bearer-token", "token_env": "GMAIL_API_KEY", "domain": "gmail.googleapis.com"}]
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
> `/Users/raul/W/gvfs/.worktrees/providers/gmail/MIGRATION_PLAN.md`.
> Read it for provider-specific shape, path tables, gotchas, and per-file
> migration notes. Wherever a passage conflicts with the corrections above
> (auth handling, POST shape, error constructors, rest captures, destructive
> action for crates-io), the corrections win.

# Gmail provider migration plan

Migrate `providers/gmail` from the pre-redesign mount-table / entity-projection SDK
to the path-first, handler-based SDK shipped on `main` at commit `6343486`.

This plan is written for a Sonnet-class executor. Every step inlines the exact
code to write. Do not invent fallbacks, do not introduce TTLs, LRUs, or a
provider-side cache, do not reintroduce preview1 adapter, and do not file
follow-up issues for anything this plan already covers.

## Summary

- Worktree: `/Users/raul/W/gvfs/.worktrees/providers/gmail`
- Worktree tip: `e1d0b85` on branch `wip/provider-gmail-impl`
- Fork point with `main`: `7742e99`
- Main tip: `6343486`
- The worktree was branched before the big provider SDK redesign landed on
  main (`refactor!: redesign provider SDK and host runtime around path-first
  handlers and callouts`). Every other work on this branch (mount-table DNS,
  mount-table GitHub) is obsoleted by main and must be discarded when we sync.

The gmail provider in the worktree currently uses:

- `omnifs_sdk::mounts! { "/..." (dir) => Struct; ... }` to register mounts
- `impl Dir for Struct` traits with `load` + `project` split
- `Projection<'_, Path>` (borrowed, typed path builders)
- `#[omnifs_sdk::provider] impl GmailProvider { fn init(config: Config) -> (State, ProviderInfo) { ... } }`
- No `on_event` handler (caching invalidation was out of scope)

The target SDK on main uses:

- `#[provider(mounts(crate::root::RootHandlers, ...))] impl GmailProvider { ... }`
- `#[handlers] impl RootHandlers { #[dir("/..")] fn ...; #[file("/..")] fn ...; }`
- Free-function path handlers returning owned `Projection` / `FileContent`
- `init` returns `Result<(State, ProviderInfo)>` (fallible is allowed; macro
  accepts both fallible and infallible forms, see github vs dns providers)
- `on_event` is an async method returning `Result<EventOutcome>`
- Provider-side caching / TTLs are forbidden; invalidation only via
  `EventOutcome::invalidate_path` / `invalidate_prefix`

## Current path table (old, to be replaced)

All paths are `dir`. No files registered directly; every file is a static
child emitted by a parent `Dir::project`.

| Path | Static child files emitted by project |
|------|----------------------------------------|
| `/` | `email-address`, `history-id`, `messages-total`, `threads-total` |
| `/labels` | children only (no files) |
| `/labels/{label_id}` | `id`, `name`, `type`, `messages-total`, `threads-total` |
| `/labels/{label_id}/threads` | children only |
| `/labels/{label_id}/threads/{thread_id}` | `subject`, `snippet`, `history-id` |
| `/labels/{label_id}/threads/{thread_id}/messages` | children only |
| `/labels/{label_id}/threads/{thread_id}/messages/{message_id}` | `subject`, `snippet`, `headers.json`, `text.txt` |
| `/threads` | children only |
| `/threads/{thread_id}` | `subject`, `snippet`, `history-id` |
| `/threads/{thread_id}/messages` | children only |
| `/threads/{thread_id}/messages/{message_id}` | `subject`, `snippet`, `headers.json`, `text.txt` |
| `/messages` | children only |
| `/messages/{message_id}` | `subject`, `snippet`, `headers.json`, `text.txt` |

## Target path table (new SDK)

We keep the same URL-visible tree, re-expressed as path-first handlers. The
new SDK derives static sibling shape from declared exact handlers, so we
declare an explicit `#[dir]` / `#[file]` per path rather than hand-writing a
`project` body that calls `p.file(...)`. For messages, files that may exceed
the 64 KiB eager budget use `file_with_stat` + lazy read.

Handler module layout (created in step 6):

- `src/root.rs`: `RootHandlers` covers `/`, `/labels`, `/threads`, `/messages`.
- `src/labels.rs`: `LabelHandlers` covers `/labels/{label_id}` and its static
  scalar files.
- `src/threads.rs`: `ThreadHandlers` covers `/threads/{thread_id}` and
  `/labels/{label_id}/threads/{thread_id}` subtrees, their `messages` sub-dir,
  and per-thread scalar files.
- `src/messages.rs`: `MessageHandlers` covers message dirs and their scalar /
  byte files in all three mount prefixes (messages-root, thread-scoped,
  label+thread-scoped).

| Handler | Kind | Path template |
|---------|------|---------------|
| `RootHandlers::root_dir` | `dir` | `/` |
| `RootHandlers::root_email_address` | `file` | `/email-address` |
| `RootHandlers::root_history_id` | `file` | `/history-id` |
| `RootHandlers::root_messages_total` | `file` | `/messages-total` |
| `RootHandlers::root_threads_total` | `file` | `/threads-total` |
| `RootHandlers::labels_dir` | `dir` | `/labels` |
| `RootHandlers::threads_dir` | `dir` | `/threads` |
| `RootHandlers::messages_dir` | `dir` | `/messages` |
| `LabelHandlers::label_dir` | `dir` | `/labels/{label_id}` |
| `LabelHandlers::label_id` | `file` | `/labels/{label_id}/id` |
| `LabelHandlers::label_name` | `file` | `/labels/{label_id}/name` |
| `LabelHandlers::label_type` | `file` | `/labels/{label_id}/type` |
| `LabelHandlers::label_messages_total` | `file` | `/labels/{label_id}/messages-total` |
| `LabelHandlers::label_threads_total` | `file` | `/labels/{label_id}/threads-total` |
| `LabelHandlers::label_threads_dir` | `dir` | `/labels/{label_id}/threads` |
| `ThreadHandlers::thread_root_dir` | `dir` | `/threads/{thread_id}` |
| `ThreadHandlers::thread_root_subject` | `file` | `/threads/{thread_id}/subject` |
| `ThreadHandlers::thread_root_snippet` | `file` | `/threads/{thread_id}/snippet` |
| `ThreadHandlers::thread_root_history_id` | `file` | `/threads/{thread_id}/history-id` |
| `ThreadHandlers::thread_root_messages_dir` | `dir` | `/threads/{thread_id}/messages` |
| `ThreadHandlers::thread_label_dir` | `dir` | `/labels/{label_id}/threads/{thread_id}` |
| `ThreadHandlers::thread_label_subject` | `file` | `/labels/{label_id}/threads/{thread_id}/subject` |
| `ThreadHandlers::thread_label_snippet` | `file` | `/labels/{label_id}/threads/{thread_id}/snippet` |
| `ThreadHandlers::thread_label_history_id` | `file` | `/labels/{label_id}/threads/{thread_id}/history-id` |
| `ThreadHandlers::thread_label_messages_dir` | `dir` | `/labels/{label_id}/threads/{thread_id}/messages` |
| `MessageHandlers::message_root_dir` | `dir` | `/messages/{message_id}` |
| `MessageHandlers::message_root_subject` | `file` | `/messages/{message_id}/subject` |
| `MessageHandlers::message_root_snippet` | `file` | `/messages/{message_id}/snippet` |
| `MessageHandlers::message_root_headers_json` | `file` | `/messages/{message_id}/headers.json` |
| `MessageHandlers::message_root_text_txt` | `file` | `/messages/{message_id}/text.txt` |
| `MessageHandlers::message_thread_dir` | `dir` | `/threads/{thread_id}/messages/{message_id}` |
| `MessageHandlers::message_thread_subject` | `file` | `/threads/{thread_id}/messages/{message_id}/subject` |
| `MessageHandlers::message_thread_snippet` | `file` | `/threads/{thread_id}/messages/{message_id}/snippet` |
| `MessageHandlers::message_thread_headers_json` | `file` | `/threads/{thread_id}/messages/{message_id}/headers.json` |
| `MessageHandlers::message_thread_text_txt` | `file` | `/threads/{thread_id}/messages/{message_id}/text.txt` |
| `MessageHandlers::message_label_dir` | `dir` | `/labels/{label_id}/threads/{thread_id}/messages/{message_id}` |
| `MessageHandlers::message_label_subject` | `file` | `/labels/{label_id}/threads/{thread_id}/messages/{message_id}/subject` |
| `MessageHandlers::message_label_snippet` | `file` | `/labels/{label_id}/threads/{thread_id}/messages/{message_id}/snippet` |
| `MessageHandlers::message_label_headers_json` | `file` | `/labels/{label_id}/threads/{thread_id}/messages/{message_id}/headers.json` |
| `MessageHandlers::message_label_text_txt` | `file` | `/labels/{label_id}/threads/{thread_id}/messages/{message_id}/text.txt` |

Why the duplication across three prefixes: the SDK's route table matches the
longest exact template, so every file the kernel may stat must have a file
handler at its real absolute path; a handler at `/messages/{id}/subject`
does not fire for `/threads/.../messages/{id}/subject`. To keep the
duplication cheap, every duplicate path dispatches into the same shared
helper (see `message_projection` and friends in step 6).

Registration order at the `#[provider]` macro site must list the narrow
families first and the root last so that SDK pattern precedence resolves
nested prefixes correctly:

```rust
#[provider(mounts(
    crate::messages::MessageHandlers,
    crate::threads::ThreadHandlers,
    crate::labels::LabelHandlers,
    crate::root::RootHandlers,
))]
```

## SDK cheatsheet (verbatim, do not paraphrase in code)

### Provider registration

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
    oauth_access_token: String,
    #[serde(default = "default_page_size")] page_size: u32,
}
fn default_page_size() -> u32 { 50 }
```

```rust
// provider.rs
use omnifs_sdk::prelude::*;
use crate::{Config, State};

#[provider(mounts(crate::root::RootHandlers))]
impl GmailProvider {
    fn init(config: Config) -> Result<(State, ProviderInfo)> {
        Ok((State { /* ... */ }, ProviderInfo {
            name: "gmail-provider".to_string(),
            version: "0.1.0".to_string(),
            description: "...".to_string(),
        }))
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["gmail.googleapis.com".to_string()],
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
        outcome.invalidate_prefix("/_labels");
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
        p.dir("_labels");
        p.dir("_threads");
        Ok(p)
    }

    #[dir("/_labels/{label_id}")]
    async fn label_dir(cx: &DirCx<'_, State>, label_id: String) -> Result<Projection> {
        let token = cx.state(|s| s.oauth_access_token.clone());
        let bytes = cx.http()
            .get(format!("https://gmail.googleapis.com/gmail/v1/users/me/labels/{label_id}"))
            .header("Authorization", format!("Bearer {token}"))
            .send_body()
            .await?;
        let mut p = Projection::new();
        p.file_with_content("label.json", bytes);
        Ok(p)
    }

    #[file("/_threads/{thread_id}.json")]
    async fn thread_json(cx: &Cx<State>, thread_id: String) -> Result<FileContent> {
        let token = cx.state(|s| s.oauth_access_token.clone());
        let bytes = cx.http()
            .get(format!("https://gmail.googleapis.com/gmail/v1/users/me/threads/{thread_id}"))
            .header("Authorization", format!("Bearer {token}"))
            .send_body()
            .await?;
        Ok(FileContent::bytes(bytes))
    }
}
```

### Context `Cx<S>`

- `cx.state(|s| ...)`, `cx.state_mut(|s| ...)`.
- `cx.http()` builder: `.get`, `.post`, `.header(k,v)`, `.json(&body)`,
  `.send_body().await`, `.send().await`.
- `cx.git()`: `.open(url).await -> Result<GitRepoInfo { tree_ref }>` (not
  relevant for gmail).
- `join_all(futs)` for parallel callouts.

### Caching, errors

- Host owns caching. Non-zero file sizes. Use `Projection::preload`,
  `Lookup::with_sibling_files`, `FileContent::with_sibling_files`.
- Invalidation: `EventOutcome::invalidate_path` / `invalidate_prefix` in
  `on_event`. Scope / identity invalidation removed.
- Errors: `ProviderError::{not_found, invalid_input, internal,
  not_a_directory, not_a_file, unimplemented}`.

### OLD → NEW map

| OLD | NEW |
|-----|-----|
| `mounts! { ... (dir) => Struct; }` | `#[dir("/...")]` free fn in `#[handlers] impl HandlersStruct` |
| `impl Dir/File/Subtree for S` trait | Single free-function handler returning `Result<Projection/FileContent/SubtreeRef>` |
| `Projection<'_, Path>` | `Projection` (owned) via prelude |
| `materialize()` | REMOVED (folds into lookup/list + subtree handoff) |
| `routes!`, `#[lookup]`, `#[list]`, `#[read]` | REMOVED |
| `Effect` / `SingleEffect` | Replaced by `Callout` (request/response only) |
| `Effect::CacheInvalidate*` | `EventOutcome` from `on_event` (no scope/identity invalidation) |
| `Effect::Git{ListTree, ReadBlob, HeadRef, ListCachedRepos}` | `Callout::GitOpenRepo` only; host does tree walks |
| Provider LRU / TTL | FORBIDDEN |
| `ProviderResult<T>` | `omnifs_sdk::prelude::Result` |
| `entities/`, `routes.rs`, `fs.rs` trait impls | `#[handlers] impl XxxHandlers` modules |

## Step 1: Bring the worktree up to main

All the worktree's scaffolding (mount-table DNS migration, mount-table GitHub
migration) is already subsumed by main and must be discarded. The simplest
safe path is a full merge from main; you will drop almost every hand-written
delta during conflict resolution.

```bash
cd /Users/raul/W/gvfs/.worktrees/providers/gmail
git fetch origin
git merge origin/main
```

Expect conflicts in these files. Resolve by taking `origin/main` and then
re-applying only the gmail-specific additions we need on top:

| File | Resolution |
|------|------------|
| `Cargo.toml` (workspace) | Take main's version, then add `"providers/gmail"` to `workspace.members` (step 7). |
| `Cargo.lock` | Delete the conflicting chunks; regenerate via `cargo metadata --format-version=1 --offline >/dev/null` or just let the next `cargo check` rewrite it. |
| `justfile` | Take main's, then append `omnifs-provider-gmail` to each provider list (step 7). |
| `crates/host/tests/provider_routes_test.rs` | Take main's version unconditionally. The old gmail test additions referenced the pre-redesign host API and are dead. |
| `docs/provider-design-gmail.md` (untracked) | Keep as-is; it is only a design note. |
| `providers/gmail/` (untracked) | Keep for now; step 6 rewrites it in place. |
| `wit/provider.wit` | Take main's. The worktree's copy is the old `single-effect` shape and will break the SDK. |

After resolving, before committing the merge:

1. Run `cargo fmt --all --check` on the workspace root; it should pass.
2. Do NOT yet run clippy or tests; the gmail crate is still on the old API
   and will not compile until step 6 is done. Commit the merge with the old
   `providers/gmail/` source intact so the migration is incremental and the
   history is readable.

If the merge is judged too noisy, the manual alternative is:

```bash
cd /Users/raul/W/gvfs/.worktrees/providers/gmail
git checkout origin/main -- \
    crates/omnifs-sdk crates/omnifs-sdk-macros crates/omnifs-mount-schema \
    crates/host crates/cli \
    wit/provider.wit \
    justfile Cargo.toml
# regenerate Cargo.lock
rm Cargo.lock && cargo generate-lockfile
```

but this produces an uglier diff and is only recommended if `git merge`
conflicts are judged unresolvable. Prefer the merge.

## Step 2: Delete files that no longer map

In `/Users/raul/W/gvfs/.worktrees/providers/gmail/providers/gmail/src/` the
following are not individually salvageable and will be rewritten wholesale
in step 6:

- `provider.rs` (replaced)
- `routes.rs` (replaced, new modules)
- `lib.rs` (replaced)

Keep and lightly port:

- `api.rs` (the Gmail REST client abstractions, wire models, and body
  decoding logic — still valid; only the `ProviderResult` alias and one
  `cx.state(...)` access change)
- `http_ext.rs` (valid as-is; Gmail-specific header convenience)
- `types.rs` (valid as-is; ID newtypes with `FromStr`)

## Step 3: Update `src/types.rs`

No changes required. The file already uses plain `serde` and `FromStr`; it
has no SDK dependency.

## Step 4: Update `src/http_ext.rs`

No changes required. Verify it still compiles by checking imports:

```rust
use omnifs_sdk::Cx;
use omnifs_sdk::http::Request;
```

Both of these are still publicly exported from the new SDK (`Cx` via
`lib.rs`, `Request` via `crate::http`). If `use omnifs_sdk::Cx;` fails
because of re-export shuffling, change it to `use omnifs_sdk::prelude::Cx;`
— the prelude re-exports `Cx` and is the stable import surface.

## Step 5: Update `src/api.rs`

Rewrite the file with the imports and the two call sites that change. The
body-decoding and Gmail wire-model logic is unchanged.

```rust
use base64::Engine as _;
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use omnifs_sdk::prelude::*;
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::http_ext::GmailHttpExt;
use crate::types::{LabelId, MessageId, ThreadId};
use crate::{Result, State};

const API_BASE: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

pub(crate) fn client(cx: &Cx<State>) -> GmailClient<'_> {
    GmailClient::new(cx)
}

pub(crate) struct GmailClient<'cx> {
    cx: &'cx Cx<State>,
}

impl<'cx> GmailClient<'cx> {
    fn new(cx: &'cx Cx<State>) -> Self {
        Self { cx }
    }

    pub(crate) async fn get_profile(&self) -> Result<Profile> {
        self.get_json("/profile", &[]).await
    }

    pub(crate) async fn list_labels(&self) -> Result<Vec<LabelStub>> {
        let response: LabelsResponse = self.get_json("/labels", &[]).await?;
        Ok(response.labels)
    }

    pub(crate) async fn get_label(&self, label_id: &LabelId) -> Result<LabelData> {
        self.get_json(&format!("/labels/{label_id}"), &[]).await
    }

    pub(crate) async fn list_threads(
        &self,
        label_id: Option<&LabelId>,
    ) -> Result<Vec<ThreadStub>> {
        let mut query = vec![(
            "maxResults",
            self.cx
                .state(|state| state.page_size)
                .to_string(),
        )];
        if let Some(label_id) = label_id {
            query.push(("labelIds", label_id.to_string()));
        }
        let response: ThreadsResponse = self.get_json("/threads", &query).await?;
        Ok(response.threads)
    }

    pub(crate) async fn list_messages(&self) -> Result<Vec<MessageStub>> {
        let query = vec![(
            "maxResults",
            self.cx
                .state(|state| state.page_size)
                .to_string(),
        )];
        let response: MessagesResponse = self.get_json("/messages", &query).await?;
        Ok(response.messages)
    }

    pub(crate) async fn get_thread(&self, thread_id: &ThreadId) -> Result<ThreadData> {
        self.get_json(
            &format!("/threads/{thread_id}"),
            &[
                ("format", "metadata".to_string()),
                ("metadataHeaders", "Subject".to_string()),
            ],
        )
        .await
    }

    pub(crate) async fn get_message(&self, message_id: &MessageId) -> Result<MessageData> {
        self.get_json(
            &format!("/messages/{message_id}"),
            &[("format", "full".to_string())],
        )
        .await
    }

    async fn get_json<T>(&self, path: &str, query: &[(&str, String)]) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let url = build_url(path, query);
        let body = self.cx.gmail_json_get(url).send_body().await?;
        parse_model(&body)
    }
}

fn build_url(path: &str, query: &[(&str, String)]) -> String {
    let mut url = format!("{API_BASE}{path}");
    if let Some((name, value)) = query.first() {
        url.push('?');
        url.push_str(name);
        url.push('=');
        url.push_str(value);
        for (name, value) in &query[1..] {
            url.push('&');
            url.push_str(name);
            url.push('=');
            url.push_str(value);
        }
    }
    url
}

fn parse_model<T>(body: &[u8]) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_slice(body)
        .map_err(|error| ProviderError::invalid_input(format!("JSON parse error: {error}")))
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Profile {
    #[serde(rename = "emailAddress")]
    pub(crate) email_address: String,
    #[serde(rename = "historyId")]
    pub(crate) history_id: String,
    #[serde(rename = "messagesTotal", default)]
    pub(crate) messages_total: u64,
    #[serde(rename = "threadsTotal", default)]
    pub(crate) threads_total: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct LabelsResponse {
    #[serde(default)]
    labels: Vec<LabelStub>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct LabelStub {
    pub(crate) id: LabelId,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct LabelData {
    pub(crate) id: LabelId,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(rename = "type", default)]
    pub(crate) kind: String,
    #[serde(rename = "messagesTotal", default)]
    pub(crate) messages_total: u64,
    #[serde(rename = "threadsTotal", default)]
    pub(crate) threads_total: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct ThreadsResponse {
    #[serde(default)]
    threads: Vec<ThreadStub>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ThreadStub {
    pub(crate) id: ThreadId,
}

#[derive(Clone, Debug, Deserialize)]
struct MessagesResponse {
    #[serde(default)]
    messages: Vec<MessageStub>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct MessageStub {
    pub(crate) id: MessageId,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ThreadData {
    #[serde(default)]
    pub(crate) snippet: String,
    #[serde(rename = "historyId", default)]
    pub(crate) history_id: String,
    #[serde(default)]
    pub(crate) messages: Vec<ThreadMessage>,
}

impl ThreadData {
    pub(crate) fn subject(&self) -> String {
        self.messages
            .iter()
            .find_map(|message| header_value(&message.payload.headers, "Subject"))
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ThreadMessage {
    pub(crate) id: MessageId,
    #[serde(default)]
    pub(crate) payload: MessagePayload,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct MessageData {
    #[serde(default)]
    pub(crate) snippet: String,
    #[serde(default)]
    pub(crate) payload: MessagePayload,
    #[serde(rename = "sizeEstimate", default)]
    pub(crate) size_estimate: u64,
}

impl MessageData {
    pub(crate) fn subject(&self) -> String {
        header_value(&self.payload.headers, "Subject").unwrap_or_default()
    }

    pub(crate) fn headers_json(&self) -> Result<Vec<u8>> {
        serde_json::to_vec_pretty(&self.payload.headers).map_err(|error| {
            ProviderError::internal(format!("failed to serialize headers: {error}"))
        })
    }

    pub(crate) fn text_bytes(&self) -> Result<Vec<u8>> {
        extract_text_bytes(&self.payload).map(Option::unwrap_or_default)
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct MessagePayload {
    #[serde(rename = "mimeType", default)]
    pub(crate) mime_type: String,
    #[serde(default)]
    pub(crate) headers: Vec<MessageHeader>,
    #[serde(default)]
    pub(crate) body: MessageBody,
    #[serde(default)]
    pub(crate) parts: Vec<MessagePayload>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct MessageBody {
    #[serde(default)]
    data: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct MessageHeader {
    pub(crate) name: String,
    pub(crate) value: String,
}

fn header_value(headers: &[MessageHeader], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value.clone())
}

fn extract_text_bytes(payload: &MessagePayload) -> Result<Option<Vec<u8>>> {
    if payload.mime_type.eq_ignore_ascii_case("text/plain") {
        return decode_body(&payload.body);
    }

    for part in &payload.parts {
        let bytes = extract_text_bytes(part)?;
        if bytes.is_some() {
            return Ok(bytes);
        }
    }

    if payload.parts.is_empty() {
        return decode_body(&payload.body);
    }

    Ok(None)
}

fn decode_body(body: &MessageBody) -> Result<Option<Vec<u8>>> {
    let Some(data) = body.data.as_deref() else {
        return Ok(None);
    };

    URL_SAFE_NO_PAD
        .decode(data)
        .or_else(|_| URL_SAFE.decode(data))
        .map(Some)
        .map_err(|error| {
            ProviderError::invalid_input(format!("invalid Gmail body encoding: {error}"))
        })
}

#[cfg(test)]
mod tests {
    use super::{MessageBody, MessagePayload, extract_text_bytes};

    #[test]
    fn extracts_nested_text_plain_body() {
        let payload = MessagePayload {
            mime_type: "multipart/alternative".to_string(),
            headers: Vec::new(),
            body: MessageBody::default(),
            parts: vec![
                MessagePayload {
                    mime_type: "text/html".to_string(),
                    headers: Vec::new(),
                    body: MessageBody {
                        data: Some("PGI-SFRNTDwvYj4".to_string()),
                    },
                    parts: Vec::new(),
                },
                MessagePayload {
                    mime_type: "text/plain".to_string(),
                    headers: Vec::new(),
                    body: MessageBody {
                        data: Some("SGVsbG8gd29ybGQ".to_string()),
                    },
                    parts: Vec::new(),
                },
            ],
        };

        let bytes = extract_text_bytes(&payload)
            .expect("extract text")
            .expect("text body");
        assert_eq!(bytes, b"Hello world");
    }
}
```

Changes from the old file, for review:

1. `use omnifs_sdk::prelude::*;` (gives `Cx`, `ProviderError`, `Result`).
2. `use crate::{Result, State};` instead of `ProviderResult`.
3. All `ProviderResult<T>` return types become `Result<T>`.
4. `self.cx.state(|state| state.config.thread_page_size)` becomes
   `self.cx.state(|state| state.page_size)` since the new `State` flattens
   config fields (see step 6).
5. Added a `list_messages` method and `MessageStub` / `MessagesResponse`
   types to back the `/messages` top-level listing, which was previously
   empty in the old `Messages` struct and is now a first-class listing.
6. Added `size_estimate` to `MessageData` so message file-size hints can
   come from Gmail rather than defaulting to 4 KiB.

Do not rename `client(cx)` or the method shapes. Keep `GmailHttpExt::gmail_json_get`
as the only caller entry point so the `Accept: application/json` header stays
centralized.

## Step 6: Write the new `src/` tree

Everything below is a full-file replacement. Delete the existing
`provider.rs`, `routes.rs`, `lib.rs`, and create `labels.rs`, `threads.rs`,
`messages.rs`, and `root.rs` with the content given.

### `src/lib.rs`

```rust
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

//! gmail-provider: Gmail virtual filesystem provider for omnifs.
//!
//! Exposes Gmail mailbox resources (labels, threads, messages) as a
//! virtual filesystem using the omnifs provider WIT interface.

pub(crate) use omnifs_sdk::prelude::Result;

mod api;
mod http_ext;
mod labels;
mod messages;
mod provider;
mod root;
mod threads;
pub(crate) mod types;

#[derive(Clone)]
#[omnifs_sdk::config]
pub struct Config {
    #[serde(default)]
    oauth_access_token: String,
    #[serde(default = "default_page_size")]
    page_size: u32,
}

fn default_page_size() -> u32 {
    100
}

#[derive(Clone)]
pub struct State {
    pub(crate) oauth_access_token: String,
    pub(crate) page_size: u32,
}
```

Notes:

- `oauth_access_token` is part of the config so the Gmail client can attach
  `Authorization: Bearer ...`. The old provider relied on bearer tokens
  flowing through an out-of-band auth path; the path-first SDK has no such
  injection, so the token must be part of `InstanceConfig.config`. If the
  runtime already surfaces tokens through a different mechanism, adjust
  this to pull from there and drop the field, but do NOT add a
  reqwest-style credential helper. This field being `#[serde(default)]`
  allows smoke tests without a token; handlers return
  `ProviderError::permission_denied` if empty and a network call is made
  (the Gmail API returns 401, which the SDK maps to `PermissionDenied`).
- `State` is flattened: no `config: Config` nesting. Handlers and `api.rs`
  call `cx.state(|s| s.page_size)` / `s.oauth_access_token.clone()`.

### `src/provider.rs`

```rust
use omnifs_sdk::Cx;
use omnifs_sdk::prelude::*;

use crate::{Config, State};

#[provider(mounts(
    crate::messages::MessageHandlers,
    crate::threads::ThreadHandlers,
    crate::labels::LabelHandlers,
    crate::root::RootHandlers,
))]
impl GmailProvider {
    fn init(config: Config) -> Result<(State, ProviderInfo)> {
        Ok((
            State {
                oauth_access_token: config.oauth_access_token,
                page_size: config.page_size,
            },
            ProviderInfo {
                name: "gmail-provider".to_string(),
                version: "0.1.0".to_string(),
                description: "Gmail mailbox browsing via the Gmail REST API".to_string(),
            },
        ))
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["gmail.googleapis.com".to_string()],
            auth_types: vec!["bearer-token".to_string()],
            max_memory_mb: 64,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 600,
        }
    }

    async fn on_event(_cx: Cx<State>, event: ProviderEvent) -> Result<EventOutcome> {
        let mut outcome = EventOutcome::new();
        match event {
            ProviderEvent::TimerTick(_) => {
                // No Gmail history polling is wired yet. When it lands, populate
                // `outcome.invalidate_prefix("/threads")` / `"/messages"` /
                // `"/labels/.../threads"` based on `historyId` deltas.
            },
            _ => {},
        }
        Ok(outcome)
    }
}
```

### `src/http_ext.rs`

Add `Authorization: Bearer ...` header injection alongside the existing
`Accept: application/json`. Replace the file contents with:

```rust
use omnifs_sdk::Cx;
use omnifs_sdk::http::Request;

use crate::State;

pub(crate) trait GmailHttpExt {
    fn gmail_json_get(&self, url: impl Into<String>) -> Request<'_, State>;
}

impl GmailHttpExt for Cx<State> {
    fn gmail_json_get(&self, url: impl Into<String>) -> Request<'_, State> {
        let token = self.state(|state| state.oauth_access_token.clone());
        let mut req = self.http().get(url).header("Accept", "application/json");
        if !token.is_empty() {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        req
    }
}
```

### `src/root.rs`

```rust
use omnifs_sdk::prelude::*;

use crate::api::client;
use crate::types::{LabelId, MessageId, ThreadId};
use crate::{Result, State};

pub struct RootHandlers;

#[handlers]
impl RootHandlers {
    #[dir("/")]
    async fn root(cx: &DirCx<'_, State>) -> Result<Projection> {
        let profile = client(cx).get_profile().await?;
        let mut p = Projection::new();
        p.file_with_content("email-address", profile.email_address.into_bytes());
        p.file_with_content("history-id", profile.history_id.into_bytes());
        p.file_with_content("messages-total", profile.messages_total.to_string().into_bytes());
        p.file_with_content("threads-total", profile.threads_total.to_string().into_bytes());
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[file("/email-address")]
    async fn email_address(cx: &Cx<State>) -> Result<FileContent> {
        let profile = client(cx).get_profile().await?;
        Ok(FileContent::bytes(profile.email_address.into_bytes()))
    }

    #[file("/history-id")]
    async fn history_id(cx: &Cx<State>) -> Result<FileContent> {
        let profile = client(cx).get_profile().await?;
        Ok(FileContent::bytes(profile.history_id.into_bytes()))
    }

    #[file("/messages-total")]
    async fn messages_total(cx: &Cx<State>) -> Result<FileContent> {
        let profile = client(cx).get_profile().await?;
        Ok(FileContent::bytes(profile.messages_total.to_string().into_bytes()))
    }

    #[file("/threads-total")]
    async fn threads_total(cx: &Cx<State>) -> Result<FileContent> {
        let profile = client(cx).get_profile().await?;
        Ok(FileContent::bytes(profile.threads_total.to_string().into_bytes()))
    }

    #[dir("/labels")]
    async fn labels(cx: &DirCx<'_, State>) -> Result<Projection> {
        let labels = client(cx).list_labels().await?;
        let mut p = Projection::new();
        for label in labels {
            p.dir(label.id.to_string());
        }
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/threads")]
    async fn threads(cx: &DirCx<'_, State>) -> Result<Projection> {
        let threads = client(cx).list_threads(None).await?;
        let mut p = Projection::new();
        for thread in threads {
            p.dir(thread.id.to_string());
        }
        // Gmail's threads API is inherently pageable. We deliberately
        // surface a single page so the directory is non-empty; the host
        // treats the result as non-exhaustive.
        p.page(PageStatus::More(Cursor::Page(2)));
        Ok(p)
    }

    #[dir("/messages")]
    async fn messages(cx: &DirCx<'_, State>) -> Result<Projection> {
        let messages = client(cx).list_messages().await?;
        let mut p = Projection::new();
        for message in messages {
            p.dir(message.id.to_string());
        }
        p.page(PageStatus::More(Cursor::Page(2)));
        Ok(p)
    }
}

// Force the ID newtypes into the crate's symbol table so the macro
// regeneration surface reflects their use sites. No runtime effect.
#[allow(dead_code)]
fn _touch_types(_: LabelId, _: ThreadId, _: MessageId) {}
```

### `src/labels.rs`

```rust
use omnifs_sdk::prelude::*;

use crate::api::client;
use crate::types::LabelId;
use crate::{Result, State};

pub struct LabelHandlers;

#[handlers]
impl LabelHandlers {
    #[dir("/labels/{label_id}")]
    async fn label_dir(cx: &DirCx<'_, State>, label_id: LabelId) -> Result<Projection> {
        let data = client(cx).get_label(&label_id).await?;
        let mut p = Projection::new();
        p.file_with_content("id", data.id.to_string().into_bytes());
        p.file_with_content("name", data.name.into_bytes());
        p.file_with_content("type", data.kind.into_bytes());
        p.file_with_content("messages-total", data.messages_total.to_string().into_bytes());
        p.file_with_content("threads-total", data.threads_total.to_string().into_bytes());
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[file("/labels/{label_id}/id")]
    async fn label_id(cx: &Cx<State>, label_id: LabelId) -> Result<FileContent> {
        let data = client(cx).get_label(&label_id).await?;
        Ok(FileContent::bytes(data.id.to_string().into_bytes()))
    }

    #[file("/labels/{label_id}/name")]
    async fn label_name(cx: &Cx<State>, label_id: LabelId) -> Result<FileContent> {
        let data = client(cx).get_label(&label_id).await?;
        Ok(FileContent::bytes(data.name.into_bytes()))
    }

    #[file("/labels/{label_id}/type")]
    async fn label_type(cx: &Cx<State>, label_id: LabelId) -> Result<FileContent> {
        let data = client(cx).get_label(&label_id).await?;
        Ok(FileContent::bytes(data.kind.into_bytes()))
    }

    #[file("/labels/{label_id}/messages-total")]
    async fn label_messages_total(cx: &Cx<State>, label_id: LabelId) -> Result<FileContent> {
        let data = client(cx).get_label(&label_id).await?;
        Ok(FileContent::bytes(data.messages_total.to_string().into_bytes()))
    }

    #[file("/labels/{label_id}/threads-total")]
    async fn label_threads_total(cx: &Cx<State>, label_id: LabelId) -> Result<FileContent> {
        let data = client(cx).get_label(&label_id).await?;
        Ok(FileContent::bytes(data.threads_total.to_string().into_bytes()))
    }

    #[dir("/labels/{label_id}/threads")]
    async fn label_threads_dir(cx: &DirCx<'_, State>, label_id: LabelId) -> Result<Projection> {
        let threads = client(cx).list_threads(Some(&label_id)).await?;
        let mut p = Projection::new();
        for thread in threads {
            p.dir(thread.id.to_string());
        }
        p.page(PageStatus::More(Cursor::Page(2)));
        Ok(p)
    }
}
```

### `src/threads.rs`

```rust
use omnifs_sdk::Cx;
use omnifs_sdk::prelude::*;

use crate::api::{ThreadData, client};
use crate::types::{LabelId, ThreadId};
use crate::{Result, State};

pub struct ThreadHandlers;

#[handlers]
impl ThreadHandlers {
    // Thread under /threads

    #[dir("/threads/{thread_id}")]
    async fn thread_root_dir(cx: &DirCx<'_, State>, thread_id: ThreadId) -> Result<Projection> {
        let data = client(cx).get_thread(&thread_id).await?;
        Ok(thread_projection(&data))
    }

    #[file("/threads/{thread_id}/subject")]
    async fn thread_root_subject(cx: &Cx<State>, thread_id: ThreadId) -> Result<FileContent> {
        let data = client(cx).get_thread(&thread_id).await?;
        Ok(FileContent::bytes(data.subject().into_bytes()))
    }

    #[file("/threads/{thread_id}/snippet")]
    async fn thread_root_snippet(cx: &Cx<State>, thread_id: ThreadId) -> Result<FileContent> {
        let data = client(cx).get_thread(&thread_id).await?;
        Ok(FileContent::bytes(data.snippet.into_bytes()))
    }

    #[file("/threads/{thread_id}/history-id")]
    async fn thread_root_history_id(cx: &Cx<State>, thread_id: ThreadId) -> Result<FileContent> {
        let data = client(cx).get_thread(&thread_id).await?;
        Ok(FileContent::bytes(data.history_id.into_bytes()))
    }

    #[dir("/threads/{thread_id}/messages")]
    async fn thread_root_messages_dir(
        cx: &DirCx<'_, State>,
        thread_id: ThreadId,
    ) -> Result<Projection> {
        let data = client(cx).get_thread(&thread_id).await?;
        Ok(thread_messages_projection(&data))
    }

    // Thread under /labels/{label_id}/threads

    #[dir("/labels/{label_id}/threads/{thread_id}")]
    async fn thread_label_dir(
        cx: &DirCx<'_, State>,
        _label_id: LabelId,
        thread_id: ThreadId,
    ) -> Result<Projection> {
        let data = client(cx).get_thread(&thread_id).await?;
        Ok(thread_projection(&data))
    }

    #[file("/labels/{label_id}/threads/{thread_id}/subject")]
    async fn thread_label_subject(
        cx: &Cx<State>,
        _label_id: LabelId,
        thread_id: ThreadId,
    ) -> Result<FileContent> {
        let data = client(cx).get_thread(&thread_id).await?;
        Ok(FileContent::bytes(data.subject().into_bytes()))
    }

    #[file("/labels/{label_id}/threads/{thread_id}/snippet")]
    async fn thread_label_snippet(
        cx: &Cx<State>,
        _label_id: LabelId,
        thread_id: ThreadId,
    ) -> Result<FileContent> {
        let data = client(cx).get_thread(&thread_id).await?;
        Ok(FileContent::bytes(data.snippet.into_bytes()))
    }

    #[file("/labels/{label_id}/threads/{thread_id}/history-id")]
    async fn thread_label_history_id(
        cx: &Cx<State>,
        _label_id: LabelId,
        thread_id: ThreadId,
    ) -> Result<FileContent> {
        let data = client(cx).get_thread(&thread_id).await?;
        Ok(FileContent::bytes(data.history_id.into_bytes()))
    }

    #[dir("/labels/{label_id}/threads/{thread_id}/messages")]
    async fn thread_label_messages_dir(
        cx: &DirCx<'_, State>,
        _label_id: LabelId,
        thread_id: ThreadId,
    ) -> Result<Projection> {
        let data = client(cx).get_thread(&thread_id).await?;
        Ok(thread_messages_projection(&data))
    }
}

fn thread_projection(data: &ThreadData) -> Projection {
    let mut p = Projection::new();
    p.file_with_content("subject", data.subject().into_bytes());
    p.file_with_content("snippet", data.snippet.clone().into_bytes());
    p.file_with_content("history-id", data.history_id.clone().into_bytes());
    p.page(PageStatus::Exhaustive);
    p
}

fn thread_messages_projection(data: &ThreadData) -> Projection {
    let mut p = Projection::new();
    for message in &data.messages {
        p.dir(message.id.to_string());
    }
    p.page(PageStatus::Exhaustive);
    p
}
```

### `src/messages.rs`

Gmail messages can be arbitrarily large (attachments, long bodies). Per the
SDK, projected file bytes are capped at 64 KiB; exceeding the cap causes
`Projection::file_with_content` to silently drop the entry into an error.
We therefore expose file handlers individually rather than pre-materializing
full bytes in the `#[dir]` projection. Scalar fields (`subject`, `snippet`)
stay inline in the projection because they are always small; `headers.json`
and `text.txt` expose size hints via `file_with_stat` on the dir and resolve
real bytes on `#[file]` lookups. The shared helpers live at the bottom of
this file and are reused across all three mount prefixes.

```rust
use std::num::NonZeroU64;

use omnifs_sdk::Cx;
use omnifs_sdk::prelude::*;

use crate::api::{MessageData, client};
use crate::types::{LabelId, MessageId, ThreadId};
use crate::{Result, State};

pub struct MessageHandlers;

#[handlers]
impl MessageHandlers {
    // Message under /messages

    #[dir("/messages/{message_id}")]
    async fn message_root_dir(
        cx: &DirCx<'_, State>,
        message_id: MessageId,
    ) -> Result<Projection> {
        let data = client(cx).get_message(&message_id).await?;
        message_projection(&data)
    }

    #[file("/messages/{message_id}/subject")]
    async fn message_root_subject(cx: &Cx<State>, message_id: MessageId) -> Result<FileContent> {
        let data = client(cx).get_message(&message_id).await?;
        Ok(FileContent::bytes(data.subject().into_bytes()))
    }

    #[file("/messages/{message_id}/snippet")]
    async fn message_root_snippet(cx: &Cx<State>, message_id: MessageId) -> Result<FileContent> {
        let data = client(cx).get_message(&message_id).await?;
        Ok(FileContent::bytes(data.snippet.into_bytes()))
    }

    #[file("/messages/{message_id}/headers.json")]
    async fn message_root_headers_json(
        cx: &Cx<State>,
        message_id: MessageId,
    ) -> Result<FileContent> {
        let data = client(cx).get_message(&message_id).await?;
        Ok(FileContent::bytes(data.headers_json()?))
    }

    #[file("/messages/{message_id}/text.txt")]
    async fn message_root_text_txt(
        cx: &Cx<State>,
        message_id: MessageId,
    ) -> Result<FileContent> {
        let data = client(cx).get_message(&message_id).await?;
        Ok(FileContent::bytes(data.text_bytes()?))
    }

    // Message under /threads/{thread_id}/messages

    #[dir("/threads/{thread_id}/messages/{message_id}")]
    async fn message_thread_dir(
        cx: &DirCx<'_, State>,
        _thread_id: ThreadId,
        message_id: MessageId,
    ) -> Result<Projection> {
        let data = client(cx).get_message(&message_id).await?;
        message_projection(&data)
    }

    #[file("/threads/{thread_id}/messages/{message_id}/subject")]
    async fn message_thread_subject(
        cx: &Cx<State>,
        _thread_id: ThreadId,
        message_id: MessageId,
    ) -> Result<FileContent> {
        let data = client(cx).get_message(&message_id).await?;
        Ok(FileContent::bytes(data.subject().into_bytes()))
    }

    #[file("/threads/{thread_id}/messages/{message_id}/snippet")]
    async fn message_thread_snippet(
        cx: &Cx<State>,
        _thread_id: ThreadId,
        message_id: MessageId,
    ) -> Result<FileContent> {
        let data = client(cx).get_message(&message_id).await?;
        Ok(FileContent::bytes(data.snippet.into_bytes()))
    }

    #[file("/threads/{thread_id}/messages/{message_id}/headers.json")]
    async fn message_thread_headers_json(
        cx: &Cx<State>,
        _thread_id: ThreadId,
        message_id: MessageId,
    ) -> Result<FileContent> {
        let data = client(cx).get_message(&message_id).await?;
        Ok(FileContent::bytes(data.headers_json()?))
    }

    #[file("/threads/{thread_id}/messages/{message_id}/text.txt")]
    async fn message_thread_text_txt(
        cx: &Cx<State>,
        _thread_id: ThreadId,
        message_id: MessageId,
    ) -> Result<FileContent> {
        let data = client(cx).get_message(&message_id).await?;
        Ok(FileContent::bytes(data.text_bytes()?))
    }

    // Message under /labels/{label_id}/threads/{thread_id}/messages

    #[dir("/labels/{label_id}/threads/{thread_id}/messages/{message_id}")]
    async fn message_label_dir(
        cx: &DirCx<'_, State>,
        _label_id: LabelId,
        _thread_id: ThreadId,
        message_id: MessageId,
    ) -> Result<Projection> {
        let data = client(cx).get_message(&message_id).await?;
        message_projection(&data)
    }

    #[file("/labels/{label_id}/threads/{thread_id}/messages/{message_id}/subject")]
    async fn message_label_subject(
        cx: &Cx<State>,
        _label_id: LabelId,
        _thread_id: ThreadId,
        message_id: MessageId,
    ) -> Result<FileContent> {
        let data = client(cx).get_message(&message_id).await?;
        Ok(FileContent::bytes(data.subject().into_bytes()))
    }

    #[file("/labels/{label_id}/threads/{thread_id}/messages/{message_id}/snippet")]
    async fn message_label_snippet(
        cx: &Cx<State>,
        _label_id: LabelId,
        _thread_id: ThreadId,
        message_id: MessageId,
    ) -> Result<FileContent> {
        let data = client(cx).get_message(&message_id).await?;
        Ok(FileContent::bytes(data.snippet.into_bytes()))
    }

    #[file("/labels/{label_id}/threads/{thread_id}/messages/{message_id}/headers.json")]
    async fn message_label_headers_json(
        cx: &Cx<State>,
        _label_id: LabelId,
        _thread_id: ThreadId,
        message_id: MessageId,
    ) -> Result<FileContent> {
        let data = client(cx).get_message(&message_id).await?;
        Ok(FileContent::bytes(data.headers_json()?))
    }

    #[file("/labels/{label_id}/threads/{thread_id}/messages/{message_id}/text.txt")]
    async fn message_label_text_txt(
        cx: &Cx<State>,
        _label_id: LabelId,
        _thread_id: ThreadId,
        message_id: MessageId,
    ) -> Result<FileContent> {
        let data = client(cx).get_message(&message_id).await?;
        Ok(FileContent::bytes(data.text_bytes()?))
    }
}

/// Build a projection for a message directory.
///
/// `subject` and `snippet` are always inlined because they are small. The
/// larger `headers.json` and `text.txt` expose a size hint via
/// `file_with_stat` but do not carry their bytes so the 64 KiB projection
/// budget is not exhausted by a single oversized message. If a payload is
/// small, the host will still serve it directly from the sibling_files
/// the file handler attaches (`with_sibling_files`, not added here for
/// brevity; add it if profiling shows excessive round trips).
fn message_projection(data: &MessageData) -> Result<Projection> {
    let mut p = Projection::new();
    p.file_with_content("subject", data.subject().into_bytes());
    p.file_with_content("snippet", data.snippet.clone().into_bytes());

    let headers_bytes = data.headers_json()?;
    let text_bytes = data.text_bytes()?;
    push_file_hint(&mut p, "headers.json", headers_bytes.len(), Some(headers_bytes));
    push_file_hint(&mut p, "text.txt", text_bytes.len(), Some(text_bytes));

    p.page(PageStatus::Exhaustive);
    Ok(p)
}

/// Project a file with eager bytes when small, or a size-hinted placeholder
/// when it would blow the 64 KiB projection budget.
fn push_file_hint(
    p: &mut Projection,
    name: &str,
    real_size: usize,
    bytes: Option<Vec<u8>>,
) {
    const EAGER_LIMIT: usize = 64 * 1024;
    let hint = NonZeroU64::new(u64::try_from(real_size).unwrap_or(4096))
        .unwrap_or_else(|| NonZeroU64::new(4096).expect("literal is non-zero"));
    match bytes {
        Some(bytes) if bytes.len() < EAGER_LIMIT => p.file_with_content(name, bytes),
        _ => p.file_with_stat(name, FileStat { size: hint }),
    }
}
```

### `src/types.rs`

No changes. Keep the file as-is.

## Step 7: Cargo wiring

### Workspace `Cargo.toml`

After the merge from step 1, the workspace members list must include
`providers/gmail`. The merged file should read:

```toml
[workspace]
resolver = "2"
members = ["crates/*", "providers/github", "providers/dns", "providers/gmail", "providers/test"]
default-members = ["crates/cli", "crates/host"]
```

No other changes to the workspace `Cargo.toml` are required; Gmail uses
only `base64` and `serde`, no workspace-level additions.

### Provider `Cargo.toml`

Replace `providers/gmail/Cargo.toml` with:

```toml
[package]
name = "omnifs-provider-gmail"
version = "0.1.0"
edition = "2024"
description = "OmnIFS provider for Gmail mailbox browsing"
license = "MIT OR Apache-2.0"
repository = "https://github.com/raulk/omnifs"
homepage = "https://github.com/raulk/omnifs"

[lib]
crate-type = ["cdylib", "lib"]

[dependencies]
base64 = "0.22"
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

This is identical to the existing file; list it here for completeness so
the executor does not leave a stale `hashbrown` or `strum` dep drifting in
from a copy of `providers/github/Cargo.toml`.

### `justfile`

After the merge, add `omnifs-provider-gmail` to the three `check-providers`
/ `build-providers` command lists:

```make
check-providers:
    cargo check -p omnifs-provider-github -p omnifs-provider-dns -p omnifs-provider-gmail -p test-provider --target wasm32-wasip2
    cargo clippy -p omnifs-provider-github -p omnifs-provider-dns -p omnifs-provider-gmail -p test-provider --target wasm32-wasip2 -- -D warnings
    cargo test -p omnifs-provider-github -p omnifs-provider-dns -p omnifs-provider-gmail -p test-provider --target wasm32-wasip2 --no-run

build-providers:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --target wasm32-wasip2 --release \
        -p omnifs-provider-github -p omnifs-provider-dns -p omnifs-provider-gmail -p test-provider
```

## Step 8: Event-handling migration

The old provider had no `on_event` and emitted no cache invalidations. The
new provider installs a stub `on_event` that accepts `TimerTick` but does
nothing (see step 6, `src/provider.rs`). When (if) Gmail history deltas are
wired:

- TimerTick handler fetches `users.history.list` starting from the last
  `historyId` (stored via `cx.state_mut`).
- For each change, call `outcome.invalidate_prefix(...)` on the affected
  path:
  - Label change: `invalidate_prefix("/labels")` (or a specific label prefix).
  - Thread add / delete / label-change: `invalidate_prefix("/threads")`
    plus any `/labels/{label_id}/threads` prefixes.
  - Message add / delete: `invalidate_prefix("/messages")` plus its parent
    thread prefixes.
- Do NOT emit `CacheInvalidateScope` or `CacheInvalidateIdentity`; those
  variants are gone from the WIT.
- Do NOT build a provider-side LRU or TTL of historyId; store one string in
  `State` (`last_history_id: Option<String>`) and let the host own
  invalidation.

If you adopt this, extend `State`:

```rust
#[derive(Clone)]
pub struct State {
    pub(crate) oauth_access_token: String,
    pub(crate) page_size: u32,
    pub(crate) last_history_id: Option<String>,
}
```

and mirror the GitHub provider's `events.rs` shape (parallel fetch via
`join_all`, `cx.active_paths(mount_id, parse)`, etag-style short-circuiting
via `If-None-Match`). That work is out of scope for this migration; the
stub `on_event` in step 6 is sufficient to compile and match the parity of
the old provider's invalidation story (i.e. none).

## Step 9: Verification

Run in order from the worktree root:

```bash
cd /Users/raul/W/gvfs/.worktrees/providers/gmail

cargo fmt --all --check
cargo clippy -p omnifs-provider-gmail --target wasm32-wasip2 -- -D warnings
cargo test -p omnifs-provider-gmail --target wasm32-wasip2 --no-run
just check-providers
```

Expected state:

- `fmt --check`: clean.
- `clippy -D warnings`: clean. If clippy flags `needless_pass_by_value` on a
  handler receiving `LabelId` / `ThreadId` / `MessageId` by value, that is
  expected for SDK-dispatched handlers and the crate's `Cargo.toml` already
  allows it.
- `test --no-run`: the `api.rs` `extracts_nested_text_plain_body` test
  compiles, and no other tests are added.
- `just check-providers`: all four providers (github, dns, gmail,
  test-provider) compile, clippy-clean, and compile-check their tests.

If `just check` is run at workspace scope, expect the host tests
(`cargo test -p omnifs-host`) to pass unchanged. The migration does not
touch host code.

## Step 10: Risks and gmail-specific gotchas

1. **OAuth token handling.** Config's `oauth_access_token` is stored as a
   plain `String` in `State`. The runtime currently has no token refresh
   path; a 401 surfaces as `ProviderError::permission_denied`. Do not
   implement client-side refresh here; add a follow-up only if the host
   grows a token-broker callout. For local testing, put a short-lived
   OAuth access token in the instance config JSON under
   `"oauth_access_token": "..."`.

2. **Large message payloads and the 64 KiB eager limit.**
   `Projection::file_with_content` silently drops any entry whose bytes
   exceed 64 KiB and records an error on the projection, which the SDK
   translates to `invalid_input` at list time. Gmail messages with long
   bodies or quoted replies regularly exceed this limit. The plan handles
   this via `push_file_hint` in `messages.rs`: inline if under 64 KiB,
   otherwise emit a `file_with_stat` size hint and let the `#[file]`
   handler resolve real bytes on read. If future profiling shows excess
   round trips, swap to `FileContent::with_sibling_files` on the `#[file]`
   handler to piggy-back the other message files on the first read.

3. **`headers.json` can also exceed the limit on unusual messages.** The
   same `push_file_hint` path covers it. Do not trust `MessageData.size_estimate`
   for `headers.json` specifically; it is the whole-message size. For
   `text.txt`, `size_estimate` is an upper bound and fine to use as a hint
   when the decoded body is still deferred. The code in step 6 currently
   decodes eagerly because the Gmail client fetches `format=full` anyway;
   the only gain from lazy decode would be skipping base64 work, which is
   not worth the code path split.

4. **Pagination is not wired for `/threads`, `/messages`, or label thread
   listings.** Step 6 hard-codes `maxResults=config.page_size` and emits
   `PageStatus::More(Cursor::Page(2))` to tell the host the listing is not
   exhaustive, but there is no `DirCx::intent()` branch to resume with
   `pageToken`. Adding real pagination means:
   - Reading `DirIntent::List { cursor }` in the dir handler.
   - Passing the Gmail `nextPageToken` as `Cursor::Opaque(token)` when the
     page is not the last.
   - Calling `client.list_threads_page(token)` on resumption.

   Do this as a follow-up only if directory listings truncate at 100
   entries in practice. Track the parity with GitHub's `numbered::search`.

5. **Attachments and MIME decoding.** The current provider surfaces only
   `text.txt` (the first `text/plain` part). Attachments are ignored.
   Multipart/alternative is flattened by picking the first `text/plain`
   that exists; `text/html` parts are not converted to plain text. This
   matches the old behavior. Adding a `parts/{index}/` subtree per message
   is a separate feature; do not add it in this migration.

6. **Path parameter types.** The old `mounts!` macro used `capture
   label_id: LabelId` syntactically; the new `#[dir("/labels/{label_id}")]`
   infers the capture from the function argument named `label_id` of type
   `LabelId`. `LabelId`, `ThreadId`, `MessageId` already implement
   `FromStr`, which is what the SDK requires for path captures. Do not
   rename the newtypes; keep `FromStr::Err = ()` — the SDK maps parse
   failure to a 404.

7. **The `#[omnifs_sdk::config]` attribute.** The old code had
   `#[derive(Clone)] #[omnifs_sdk::config] pub struct Config`. The order of
   `#[derive]` and `#[omnifs_sdk::config]` matters: the `config` macro
   emits `Serialize`/`Deserialize` derives and the config receiver
   wire-up. Keep `#[omnifs_sdk::config]` as the outer-most attribute on
   the `Config` struct, as in `providers/dns/src/lib.rs` and
   `providers/github/src/lib.rs`. `#[derive(Clone)]` is fine above it.

8. **Worktree branch name.** The branch is `wip/provider-gmail-impl`. When
   commits land, keep the name. Do not force-push to `main`.

## Decision log

- Chose to keep the full three-mount tree (`/labels/{id}/threads/{id}/messages/{id}`,
  `/threads/{id}/messages/{id}`, `/messages/{id}`) rather than collapse to
  one canonical view, because the old provider exposed all three and the
  filesystem tree is part of the user-facing contract. The duplication is
  mechanical and compiles into a fixed registry at init time; there is no
  per-request overhead.
- Chose to put `oauth_access_token` in `Config` + `State`. The SDK has no
  bearer-token callout; `capabilities.auth_types = ["bearer-token"]` is
  advisory only. Pulling the token through `State` is consistent with how
  DNS stores its resolver list.
- Chose `refresh_interval_secs: 600` (10 minutes) to match a reasonable
  polling cadence for Gmail when `on_event` grows a real body; the stub
  handler in step 6 accepts the ticks without acting on them, which is
  harmless.
- Chose not to add `#[mutate]` handlers. Gmail is read-only in this
  provider.

---

## Verification

- `cargo fmt --check`
- `cargo clippy -p omnifs-provider-gmail --target wasm32-wasip2 -- -D warnings`
- `cargo test -p omnifs-provider-gmail --target wasm32-wasip2 --no-run`
- `just check-providers`

All must pass. If `just` is not on PATH, note that in the PR body and run
the equivalent `cargo` commands from the root of this branch's worktree.

## Commit

Conventional:

```
feat(gmail): migrate provider to path-first handler SDK
```

Body: one paragraph naming the major structural changes and the base SDK
PRs (#28 `feat/sdk-http-post-support`, #29 `feat/sdk-path-rest-captures`,
#D `feat/sdk-error-constructors`).

## PR

- Title: `feat(gmail): migrate provider to path-first handler SDK`
- Body: summary + link to this branch's `PLAN.md` + verification results
  (which cargo commands ran, which passed, which were skipped and why).
