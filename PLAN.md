# feat/migrate-linear

The linear provider (~3,100 LoC, largest in the tree) was built against the pre-6343486 SDK.

## Blocked by

This plan cannot start execution until both of these have merged into `main`:

- PR #28 `feat/sdk-http-post-support` — https://github.com/raulk/omnifs/pull/28
- PR #29 `feat/sdk-path-rest-captures` — https://github.com/raulk/omnifs/pull/29

Note: `ProviderError::rate_limited` / `::permission_denied` / `::version_mismatch`
constructors are already on `main` (landed with the #27 refactor). No separate PR
is needed.

## Execution model

This branch was created off `main` at `6343486`. To execute:

1. `git -C /Users/raul/W/gvfs worktree add /Users/raul/W/gvfs/.worktrees/migrate-linear feat/migrate-linear`
2. Work in that worktree only.
3. Bring in the provider source from the old worktree at
   `/Users/raul/W/gvfs/.worktrees/providers/linear/providers/linear/`
   per the "Port provider source" step below.
4. Execute this PLAN.md end-to-end. Corrections in the "Migration
   corrections" section are authoritative over anything in the reference
   body that contradicts them.
5. Run the Verification commands listed near the bottom.
6. Commit on the `feat/migrate-linear` branch, push, open PR.


## Port provider source

This branch is off `main` at `6343486`, so there is NO merge from
`wip/provider-linear-impl` and NO `git merge main`. The wip branch carries OLD-SDK infrastructure
that must not land here. Only provider-local files come over, file by file,
using `git checkout <old-branch> -- <path>` (this pulls the file contents into
the working tree and index without touching anything else).

### Files to copy verbatim (no touch-ups beyond rust import paths / `ProviderResult` → `Result`)

- `providers/linear/src/types.rs`

Bring each over with:

```bash
git checkout wip/provider-linear-impl -- providers/linear/src/types.rs
```

### Files to copy then touch up

- `providers/linear/src/api.rs` — copy over, then apply the corrections in
  the relevant reference-body sections (auth removal, POST rewrites, error
  constructor substitutions).
- `providers/linear/src/events.rs` — copy over, then apply the corrections in
  the relevant reference-body sections (auth removal, POST rewrites, error
  constructor substitutions).

Bring them in with:

```bash
git checkout wip/provider-linear-impl -- providers/linear/src/api.rs
git checkout wip/provider-linear-impl -- providers/linear/src/events.rs
```

Then edit in place.

### Files to create fresh (do NOT copy from the wip branch)

- `providers/linear/src/lib.rs`
- `providers/linear/src/provider.rs`
- `providers/linear/src/root.rs`
- `providers/linear/src/handlers/ (teams, issues, cycles, projects, users)`

### Files to DISCARD (do NOT bring to this branch)

- `providers/linear/src/http_ext.rs`
- `providers/linear/src/entities/ (entire folder)`
- `providers/linear/src/old provider.rs`
- `providers/linear/src/old lib.rs`

These are old-SDK artifacts (entity projections, tree walkers, routes tables,
manual http_ext wrappers for auth). The new SDK shape replaces them with
path-first handlers.

### Bring over the provider Cargo.toml

```bash
git checkout wip/provider-linear-impl -- providers/linear/Cargo.toml
```

Then update its SDK dependency declarations to match `providers/github/Cargo.toml`
on the current `main`. In particular, `omnifs-sdk` must point at the workspace
version and not an old path/git revision.

### Re-register the provider in the workspace

The workspace-level `Cargo.toml` on `main` dropped every non-dns/github/test
provider. Re-add `providers/linear` to its `members` array. Example diff:

```toml
[workspace]
members = [
    "crates/cli",
    "crates/host",
    "providers/dns",
    "providers/github",
+   "providers/linear",
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
    domains: vec!["api.linear.app".to_string()],
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

  - `api.linear.app`

Mount config shape the user supplies:

```json
{
  "plugin": "linear.wasm",
  "mount": "/linear",
  "auth": [{"type": "bearer-token", "token_env": "LINEAR_API_KEY", "domain": "api.linear.app"}]
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
> `/Users/raul/W/gvfs/.worktrees/providers/linear/MIGRATION_PLAN.md`.
> Read it for provider-specific shape, path tables, gotchas, and per-file
> migration notes. Wherever a passage conflicts with the corrections above
> (auth handling, POST shape, error constructors, rest captures, destructive
> action for crates-io), the corrections win.

# Linear provider migration plan

## Summary

The linear provider (~3,100 LoC, largest in the tree) was built against the
pre-6343486 SDK. It uses `mounts! { ... }` + trait-based `Dir` / `Subtree` /
`routes!` handlers, provider-orchestrated cache invalidation
(`CacheInvalidateIdentity/Prefix/Scope`), scope/identity cache keys, and a
full mutation pipeline (`plan_mutations`, `execute`, `fetch_resource`,
`list_scope`). None of those concepts exist on main: path-first free-function
handlers, callouts as the only suspension primitive, host-owned caching,
event-driven invalidation via `on_event` + `EventOutcome`.

This plan migrates the provider in place inside the worktree
`.worktrees/providers/linear`. We merge main, drop the pre-refactor API
surface, rewrite the seven handler families (`root`, `team`, `issue`,
`project`, `cycle`, `label`, `workflow_state`, `comment`) as
`#[handlers] impl` blocks with free-function `#[dir]` / `#[file]` /
`#[subtree]` handlers, fold scope/identity invalidation into an
etag-polled `on_event(TimerTick)` that mirrors `providers/github/src/events.rs`,
and defer the reconcile/mutations surface (the new WIT still has
`plan-mutations` / `execute` / `fetch-resource` arms, but no SDK helpers
and no reference implementation land on main; see the "Deferred surfaces"
section for the explicit cut).

Two infrastructure items on main are insufficient for linear as written
and must be added inside this worktree (see "Required SDK additions"):

1. `omnifs_sdk::http::Builder::post(url)` and `Request::body(...)` /
   `Request::json(&T)`. The current builder only exposes `.get()` and
   `.header()` / `.send()` / `.send_body()`; Linear's provider is 100%
   POST GraphQL, so every call needs POST + JSON body. The underlying
   `Callout::Fetch(HttpRequest { method, url, headers, body })` already
   supports this, it just isn't wired through the builder.
2. `omnifs_sdk::prelude::Result` re-export of a `Default` config when
   the config is empty. Not a blocker; the linear config has fields.

---

## Current path table (verbatim from old `lib.rs` `mounts!`)

```
capture team:    crate::types::TeamKey
capture issue:   crate::types::IssueKey
capture project: crate::types::UuidKey
capture cycle:   crate::types::UuidKey
capture label:   crate::types::UuidKey
capture state:   crate::types::UuidKey
capture comment: crate::types::UuidKey

"/"                                                      (dir)     => Root
"/.linear"                                               (subtree) => ControlTree
"/_projects"                                             (dir)     => Projects
"/_projects/{project}"                                   (dir)     => Project
"/_projects/{project}/issues"                            (dir)     => ProjectIssues
"/_projects/{project}/issues/{issue}"                    (dir)     => ProjectIssue
"/{team}"                                                (dir)     => Team
"/{team}/issues"                                         (dir)     => Issues
"/{team}/issues/{issue}"                                 (dir)     => Issue
"/{team}/issues/{issue}/comments"                        (dir)     => Comments
"/{team}/issues/{issue}/comments/{comment}"              (dir)     => Comment
"/{team}/issues/{issue}/.draft"                          (subtree) => IssueDraftTree
"/{team}/issues/{issue}/comments/.draft"                 (subtree) => CommentDraftTree
"/{team}/issues/.draft"                                  (subtree) => NewIssueDraftTree
"/{team}/projects"                                       (dir)     => TeamProjects
"/{team}/projects/{project}"                             (dir)     => ProjectAlias
"/{team}/cycles"                                         (dir)     => Cycles
"/{team}/cycles/{cycle}"                                 (dir)     => Cycle
"/{team}/cycles/{cycle}/issues"                          (dir)     => CycleIssues
"/{team}/cycles/{cycle}/issues/{issue}"                  (dir)     => CycleIssue
"/{team}/labels"                                         (dir)     => Labels
"/{team}/labels/{label}"                                 (dir)     => Label
"/{team}/labels/{label}/issues"                          (dir)     => LabelIssues
"/{team}/labels/{label}/issues/{issue}"                  (dir)     => LabelIssue
"/{team}/workflow_states"                                (dir)     => WorkflowStates
"/{team}/workflow_states/{state}"                        (dir)     => WorkflowState
"/{team}/workflow_states/{state}/issues"                 (dir)     => WorkflowStateIssues
"/{team}/workflow_states/{state}/issues/{issue}"         (dir)     => WorkflowStateIssue
```

---

## Target path table

Capture types all implement `FromStr` in `src/types.rs` today; the
macro resolves non-`String` captures via `FromStr`. Reuse the existing
newtypes as-is.

| Path                                                      | Kind     | Capture types                                                  | Handler fn (target)                         |
|-----------------------------------------------------------|----------|----------------------------------------------------------------|---------------------------------------------|
| `/`                                                       | dir      | —                                                              | `RootHandlers::root`                        |
| `/.linear`                                                | subtree  | —                                                              | DEFERRED (see "Deferred surfaces")          |
| `/_projects`                                              | dir      | —                                                              | `ProjectHandlers::all_projects`             |
| `/_projects/{project}`                                    | dir      | `project: UuidKey`                                             | `ProjectHandlers::project`                  |
| `/_projects/{project}/issues`                             | dir      | `project: UuidKey`                                             | `ProjectHandlers::project_issues`           |
| `/_projects/{project}/issues/{issue}`                     | dir      | `project: UuidKey, issue: IssueKey`                            | `ProjectHandlers::project_issue`            |
| `/{team}`                                                 | dir      | `team: TeamKey`                                                | `TeamHandlers::team`                        |
| `/{team}/issues`                                          | dir      | `team: TeamKey`                                                | `TeamHandlers::team_issues`                 |
| `/{team}/issues/{issue}`                                  | dir      | `team: TeamKey, issue: IssueKey`                               | `IssueHandlers::issue`                      |
| `/{team}/issues/{issue}/comments`                         | dir      | `team: TeamKey, issue: IssueKey`                               | `IssueHandlers::issue_comments`             |
| `/{team}/issues/{issue}/comments/{comment}`               | dir      | `team: TeamKey, issue: IssueKey, comment: UuidKey`             | `IssueHandlers::issue_comment`              |
| `/{team}/issues/{issue}/.draft`                           | subtree  | —                                                              | DEFERRED                                    |
| `/{team}/issues/{issue}/comments/.draft`                  | subtree  | —                                                              | DEFERRED                                    |
| `/{team}/issues/.draft`                                   | subtree  | —                                                              | DEFERRED                                    |
| `/{team}/projects`                                        | dir      | `team: TeamKey`                                                | `TeamHandlers::team_projects`               |
| `/{team}/projects/{project}`                              | dir      | `team: TeamKey, project: UuidKey`                              | `TeamHandlers::team_project_alias`          |
| `/{team}/cycles`                                          | dir      | `team: TeamKey`                                                | `CycleHandlers::cycles`                     |
| `/{team}/cycles/{cycle}`                                  | dir      | `team: TeamKey, cycle: UuidKey`                                | `CycleHandlers::cycle`                      |
| `/{team}/cycles/{cycle}/issues`                           | dir      | `team: TeamKey, cycle: UuidKey`                                | `CycleHandlers::cycle_issues`               |
| `/{team}/cycles/{cycle}/issues/{issue}`                   | dir      | `team: TeamKey, cycle: UuidKey, issue: IssueKey`               | `CycleHandlers::cycle_issue`                |
| `/{team}/labels`                                          | dir      | `team: TeamKey`                                                | `LabelHandlers::labels`                     |
| `/{team}/labels/{label}`                                  | dir      | `team: TeamKey, label: UuidKey`                                | `LabelHandlers::label`                      |
| `/{team}/labels/{label}/issues`                           | dir      | `team: TeamKey, label: UuidKey`                                | `LabelHandlers::label_issues`               |
| `/{team}/labels/{label}/issues/{issue}`                   | dir      | `team: TeamKey, label: UuidKey, issue: IssueKey`               | `LabelHandlers::label_issue`                |
| `/{team}/workflow_states`                                 | dir      | `team: TeamKey`                                                | `WorkflowStateHandlers::states`             |
| `/{team}/workflow_states/{state}`                         | dir      | `team: TeamKey, state: UuidKey`                                | `WorkflowStateHandlers::state`              |
| `/{team}/workflow_states/{state}/issues`                  | dir      | `team: TeamKey, state: UuidKey`                                | `WorkflowStateHandlers::state_issues`       |
| `/{team}/workflow_states/{state}/issues/{issue}`          | dir      | `team: TeamKey, state: UuidKey, issue: IssueKey`               | `WorkflowStateHandlers::state_issue`        |

Capture types validated in the existing `types.rs` and kept verbatim:
- `TeamKey` (safe segment, ASCII alphanum + `-_.`, non-dot-leading)
- `IssueKey` (`<UPPER-TEAM>-<digits>`, e.g. `FIB-67`)
- `UuidKey` (hex + `-`)

These already implement `FromStr`, so `#[dir("/{team}/issues/{issue}")]`
with `team: TeamKey, issue: IssueKey` resolves via the macro's
`FromStr` path without further changes.

---

## Deferred surfaces

Explicitly out of scope for this migration, with rationale:

1. **Drafts and the `.linear` control tree.** The old provider models
   write-side drafts (issue create/update, comment create) as a subtree
   with `.draft` handoffs plus `plan_mutations` / `execute` reconcile
   terminals. The new WIT still exports `plan-mutations`, `execute`,
   `fetch-resource` arms, but the SDK on main has no helper types for
   `FileChange`, `PlannedMutation`, `MutationOutcome`, `PostAction`
   beyond the raw wit bindgen re-exports (no handler macro, no test).
   Neither `providers/github/src/` nor `providers/dns/src/` exercises
   this path. Migrating drafts requires first adding an SDK surface
   for reconcile handlers, which is a separate workstream.

   Concretely delete: `src/entities/draft.rs`, all mutation
   orchestration in `src/api.rs` (everything below and including
   `plan_mutations`, `execute`, `execute_issue_create`, `fetch_resource`,
   `list_scope`, and the GraphQL mutation builders), and the four
   reconcile fns on `LinearProvider` (`plan_mutations`, `execute`,
   `fetch_resource`, `list_scope`).

   Keep for later: `src/types.rs` mutation records (`IssueDraft`,
   `CommentDraft`, `LinearMutationPlan`, `OP_*` constants) remain as
   dead code behind `#[cfg(feature = "mutations")]` or just deleted.
   Prefer deletion since the data model is likely to shift once the
   SDK offers a reconcile handler abstraction.

2. **Scope listings.** Old `list_scope("team:...")` / `"project:..."` /
   `"issue:..."` etc. mirror into flat `FileEntry` lists for the host's
   cache. New caching is fully path-driven through projection-based
   dir/file handlers plus `preload` / sibling-files. Delete the entire
   `list_scope` function.

3. **Identity invalidation**. Gone by design. All invalidation goes
   through path/prefix on `EventOutcome` now; `ScopeType` and
   `IdentityKey` are not imported anywhere in the new SDK prelude.
   Delete `team_identity`, `issue_identity`, `project_identity`,
   `cycle_identity`, `label_identity`, `workflow_state_identity`,
   `comment_identity`, `TeamScope`, `ProjectScope`, `IssueScope`,
   `CycleScope`, `LabelScope`, `WorkflowStateScope`, `CommentScope`
   from `types.rs`.

---

## Required SDK additions (inside this worktree only)

All changes scoped to `crates/omnifs-sdk/src/http.rs`. These must land
before any POST-based handler can compile, since every GraphQL call is
a POST with a JSON body.

### 1. POST on the builder

Extend `Builder`:

```rust
impl<'cx, S> Builder<'cx, S> {
    pub fn get(self, url: impl Into<String>) -> Request<'cx, S> {
        Request { cx: self.cx, method: "GET".to_string(), url: url.into(), headers: Vec::new(), body: None }
    }

    pub fn post(self, url: impl Into<String>) -> Request<'cx, S> {
        Request { cx: self.cx, method: "POST".to_string(), url: url.into(), headers: Vec::new(), body: None }
    }
}
```

### 2. Body and JSON on the request

Extend `Request`:

```rust
impl<'cx, S> Request<'cx, S> {
    // (existing header, send, send_body unchanged)

    #[must_use]
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Serialize `payload` as JSON, set the body, and force the
    /// `Content-Type` header if the caller has not set one.
    #[must_use]
    pub fn json<T: serde::Serialize>(mut self, payload: &T) -> Self {
        // Best-effort; serialization failure leaves body None which
        // surfaces as a 4xx from the host or a type mismatch on the
        // server. Callers can validate up front if strict encoding is
        // required.
        if let Ok(bytes) = serde_json::to_vec(payload) {
            self.body = Some(bytes);
            if !self.headers.iter().any(|h| h.name.eq_ignore_ascii_case("content-type")) {
                self.headers.push(crate::omnifs::provider::types::Header {
                    name: "Content-Type".to_string(),
                    value: "application/json".to_string(),
                });
            }
        }
        self
    }
}
```

`serde_json` is already a transitive dependency of `omnifs-sdk` (via
the config macro). No new workspace dep needed.

### Rationale

Keeping these on the core SDK, not a provider-local extension trait,
avoids every future HTTP-POST-using provider (slack, notion, jira) each
inventing their own `post_json_bytes` wrapper. This matches the symmetry
of `get` / `post` in every other HTTP crate and respects the user's
"prefer battle-tested libraries" and "composable, structurally honest"
rules.

---

## SDK cheatsheet (inline verbatim, copy into handlers without lookup)

### Provider top level

```rust
use omnifs_sdk::prelude::*;
use crate::{Config, State};

#[provider(mounts(
    crate::root::RootHandlers,
    crate::team::TeamHandlers,
    crate::issue::IssueHandlers,
    crate::project::ProjectHandlers,
    crate::cycle::CycleHandlers,
    crate::label::LabelHandlers,
    crate::workflow_state::WorkflowStateHandlers,
))]
impl LinearProvider {
    fn init(config: Config) -> Result<(State, ProviderInfo)> { /* ... */ }
    fn capabilities() -> RequestedCapabilities { /* ... */ }
    async fn on_event(cx: Cx<State>, event: ProviderEvent) -> Result<EventOutcome> {
        /* TimerTick: etag-polled invalidate_prefix */
    }
}
```

### Handler shapes

```rust
#[handlers]
impl Foo {
    #[dir("/path/{cap}")]
    async fn name(cx: &DirCx<'_, State>, cap: Cap) -> Result<Projection> { ... }

    #[file("/path/exact.json")]
    async fn file(cx: &Cx<State>) -> Result<FileContent> { ... }

    #[subtree("/path/{cap}/_tree")]
    async fn tree(cx: &Cx<State>, cap: Cap) -> Result<SubtreeRef> { ... }
}
```

Rules:
- `DirCx<'_, S>` derefs to `Cx<S>`; `cx.intent()` returns `&DirIntent`.
- `DirIntent::{Lookup { child }, List { cursor }, ReadProjectedFile { name }}`.
- Sync or `async fn`; captures typed via `FromStr` for non-`String`.

### Projection builders

```rust
let mut p = Projection::new();
p.dir("subdir");
p.file("name");                                  // placeholder size 4096
p.file_with_stat("name", FileStat { size: NonZeroU64::new(len).unwrap() });
p.file_with_content("title", title_bytes);      // eager, <= 64 KiB
p.page(PageStatus::Exhaustive);                 // or PageStatus::More(Cursor::Opaque(..)) / Cursor::Page(n)
p.preload("relative/path", bytes);              // host caches for later read
p.preload_many([("a", ab), ("b", bb)]);
```

### Lookup / FileContent builders

```rust
Lookup::entry(Entry::dir("x"))
    .with_siblings([Entry::file("y", size)])
    .with_sibling_files([ProjectedFile::new("y", y_bytes)])
    .with_preload([Preload::new("nested/z", z_bytes)])
    .exhaustive(true);

Lookup::file("title", title_bytes);             // combines Entry + sibling_files
Lookup::subtree(tree_ref);
Lookup::not_found();

FileContent::new(bytes).with_sibling_files([ProjectedFile::new("adj", adj_bytes)]);
```

### Context

```rust
cx.state(|s| s.config.page_size);
cx.state_mut(|s| { s.etags.insert(key, etag); });

cx.http().get(url).header("k", "v").send_body().await?;   // Result<Vec<u8>>
cx.http().post(url).header("k", "v").json(&body).send().await?; // Result<HttpResponse>
                                                                 // response.status, response.headers, response.body
cx.http().post(url).body(raw_bytes).send_body().await?;   // Result<Vec<u8>>

cx.active_paths(RepoPath::MOUNT_ID, |p| parse_id(p)); // Vec<Id>, filtered by mount

let results = join_all(futs).await;                // Vec<Result<T>>; single batched callout round-trip
```

### EventOutcome

```rust
let mut outcome = EventOutcome::new();
outcome.invalidate_path("/absolute/path");
outcome.invalidate_prefix("/absolute/prefix");
Ok(outcome)
```

### Errors

```rust
ProviderError::not_found(msg)
ProviderError::invalid_input(msg)
ProviderError::not_a_directory(msg)
ProviderError::not_a_file(msg)
ProviderError::internal(msg)
ProviderError::unimplemented(msg)
// No rate_limited / permission_denied / version_mismatch in Result's
// convenience API; construct via ProviderError::new or the shared
// ProviderErrorKind (see Risks below).
```

---

## Bring worktree up to main

Worktree tip: `e1d0b85` ("fix(mounts): restore projected sibling file
dispatch"). Fork point from main: `7742e99` ("docs(readme): expand
examples"). Main tip: `6343486` ("refactor!: redesign provider SDK...").

The entire SDK/WIT redesign happened in `6343486`. The worktree
independently extended the *old* SDK (adding its own mounts/projection
machinery and identity invalidation) to land the linear provider on top.
Merging main means discarding the worktree's local SDK work entirely
and taking main's SDK/WIT wholesale.

### Git sequence

```bash
cd /Users/raul/W/gvfs/.worktrees/providers/linear

# Confirm we're on a non-main branch with a clean tree.
git status

# Merge main. Expect conflicts under crates/ and wit/ where the
# worktree's SDK extensions collide with main's rewrite, and under
# Cargo.toml / Cargo.lock.
git merge main
```

### Conflict resolution (take-theirs for SDK/WIT)

For every conflict in these paths, accept main's version unchanged:

```bash
# All crates: main's SDK supersedes the worktree's local SDK work.
git checkout --theirs -- crates/
git add crates/

# All WIT: main's is the 0.2.0 redesign; take it wholesale.
git checkout --theirs -- wit/
git add wit/

# Top-level omnifs metadata owned by main.
git checkout --theirs -- \
    CLAUDE.md AGENTS.md CHANGELOG.md README.md \
    Dockerfile compose.yaml compose.ci.yaml \
    scripts/ docs/ justfile rustfmt.toml rust-toolchain.toml
git add CLAUDE.md AGENTS.md CHANGELOG.md README.md \
        Dockerfile compose.yaml compose.ci.yaml \
        scripts docs justfile rustfmt.toml rust-toolchain.toml
```

For `providers/linear/**`: keep ours (the worktree source) as the
starting point for rewrite. Don't auto-resolve there, we will rewrite
those files file-by-file below.

```bash
git checkout --ours -- providers/linear/
git add providers/linear/
```

For `Cargo.toml` (workspace root): conflicts are expected because main
does not include `providers/linear` in `members`. Hand-edit to take
main's contents and append `"providers/linear"` to members (see Cargo
section below). `Cargo.lock`: delete it and let `cargo build` regenerate.

```bash
# After hand-editing Cargo.toml:
git add Cargo.toml
rm -f Cargo.lock
```

Finish the merge:

```bash
git commit -m "merge main into providers/linear worktree"
```

At this point the tree has main's SDK, main's WIT, and the old linear
provider source untouched. It will NOT compile — that is expected and
the starting point of the rewrite.

### Sanity probe after merge

```bash
# Expect many errors in providers/linear; other providers build fine.
cargo check -p omnifs-provider-github --target wasm32-wasip2
cargo check -p omnifs-provider-dns    --target wasm32-wasip2
cargo check -p omnifs-provider-linear --target wasm32-wasip2  # fails
```

---

## Per-file migration

All paths relative to `providers/linear/`.

### `src/lib.rs` — rewrite

Delete the old `mounts!` invocation, the `entities` re-exports,
`ProviderResult`, and all `events.rs` / `http_ext.rs` legacy bits.
Replace with:

```rust
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

pub(crate) use omnifs_sdk::prelude::Result;

mod cycle;
mod events;
mod graphql;
mod http_ext;
mod issue;
mod label;
mod project;
mod provider;
mod root;
mod team;
pub(crate) mod types;
mod workflow_state;

#[derive(Clone)]
pub(crate) struct State {
    pub config: Config,
    /// Per-team etag for the `issues(filter:{updatedAt:{gt}})` polling
    /// query used from `on_event(TimerTick)`. Keyed by TeamKey string.
    pub team_issue_etags: hashbrown::HashMap<String, String>,
}

#[omnifs_sdk::config]
#[derive(Clone)]
pub struct Config {
    pub api_key: String,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    #[serde(default = "default_recent_update_limit")]
    pub recent_update_limit: u32,
}

fn default_page_size() -> u32 { 50 }
fn default_recent_update_limit() -> u32 { 100 }
```

Removed: `enable_mutations` (mutations deferred), `ProviderResult`
alias (use `Result` from prelude), every `entities::*` re-export.

### `src/provider.rs` — rewrite

Drop the four pre-refactor fns (`plan_mutations`, `execute`,
`fetch_resource`, `list_scope`) and the manual `drive()` plumbing.
Replace with:

```rust
use omnifs_sdk::prelude::*;

use crate::events::timer_tick;
use crate::{Config, State};

#[provider(mounts(
    crate::root::RootHandlers,
    crate::team::TeamHandlers,
    crate::issue::IssueHandlers,
    crate::project::ProjectHandlers,
    crate::cycle::CycleHandlers,
    crate::label::LabelHandlers,
    crate::workflow_state::WorkflowStateHandlers,
))]
impl LinearProvider {
    fn init(config: Config) -> Result<(State, ProviderInfo)> {
        Ok((
            State {
                config,
                team_issue_etags: hashbrown::HashMap::new(),
            },
            ProviderInfo {
                name: "linear-provider".to_string(),
                version: "0.1.0".to_string(),
                description: "Linear provider for omnifs".to_string(),
            },
        ))
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: vec!["api.linear.app".to_string()],
            auth_types: vec!["api-key-header".to_string(), "bearer-token".to_string()],
            max_memory_mb: 128,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 60,
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

### `src/types.rs` — rewrite

Keep `TeamKey`, `IssueKey`, `UuidKey` (the `string_key!` macro, plus the
`is_safe_segment`, `is_issue_key`, `is_uuid_like` validators). Delete:
`DraftKey`, `TransactionKey`, every `*Scope` type, every `*_identity`
fn, `IssueDraft`, `CommentDraft`, `LinearMutationPlan`, `OP_*`
constants. Final shape:

```rust
use core::str::FromStr;

macro_rules! string_key {
    ($name:ident, $validator:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub struct $name(String);

        impl FromStr for $name {
            type Err = ();
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                $validator(value).then(|| Self(value.to_string())).ok_or(())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str { &self.0 }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) }
        }
    };
}

string_key!(TeamKey, is_safe_segment);
string_key!(IssueKey, is_issue_key);
string_key!(UuidKey, is_uuid_like);

fn is_safe_segment(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('.')
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn is_issue_key(value: &str) -> bool {
    let Some((team, number)) = value.split_once('-') else { return false };
    !team.is_empty()
        && !number.is_empty()
        && team.bytes().all(|b| b.is_ascii_uppercase())
        && number.bytes().all(|b| b.is_ascii_digit())
}

fn is_uuid_like(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}
```

### `src/http_ext.rs` — rewrite

The old `post_json_bytes` depended on an `http::Builder.post().body()`
chain that no longer exists. After the SDK additions above land, this
file becomes a thin convenience layer:

```rust
use omnifs_sdk::Cx;
use omnifs_sdk::omnifs::provider::types::HttpResponse;
use omnifs_sdk::http::Request;

use crate::State;
use crate::Result;

pub(crate) const GRAPHQL_ENDPOINT: &str = "https://api.linear.app/graphql";

pub(crate) trait LinearHttpExt {
    fn linear_graphql(&self) -> Request<'_, State>;
}

impl LinearHttpExt for Cx<State> {
    fn linear_graphql(&self) -> Request<'_, State> {
        let api_key = self.state(|s| s.config.api_key.clone());
        self.http()
            .post(GRAPHQL_ENDPOINT)
            .header("Authorization", api_key)
            .header("Content-Type", "application/json")
    }
}
```

Rationale: every GraphQL call from linear is a POST to the same URL with
the same auth header. Centralising here keeps handlers free of auth
bookkeeping and mirrors `providers/github/src/http_ext.rs` (which
centralises `Accept` / `X-GitHub-Api-Version`).

### `src/graphql.rs` — new file (extracted from old `src/api.rs`)

Move all fetch fns + record types into a transport-agnostic module. The
GraphQL envelope handling does not change; only the HTTP call mechanics
do. Keep verbatim:

- Field-selection constants: `TEAM_FIELDS`, `PROJECT_FIELDS`,
  `CYCLE_FIELDS`, `LABEL_FIELDS`, `WORKFLOW_STATE_FIELDS`,
  `ISSUE_FIELDS`, `COMMENT_FIELDS`.
- Record types: `Connection<T>`, `TeamRecord`, `UserRecord`,
  `ProjectRecord`, `CycleRecord`, `LabelRecord`, `WorkflowStateRecord`,
  `CommentRecord`, `IssueRecord`.
- Internal envelope types: `GraphQlEnvelope`, `GraphQlError`,
  `GraphQlErrorExtensions`, and the per-query `*Data` shapes
  (`TeamsData`, `TeamNodeData`, `ProjectsData`, `ProjectData`,
  `CycleData`, `WorkflowStateData`, `IssuesData`, `IssueData`).
- Fetch fns: `fetch_teams`, `fetch_team`, `fetch_projects`,
  `fetch_project`, `fetch_team_projects`, `fetch_team_cycles`,
  `fetch_team_labels`, `fetch_team_workflow_states`, `fetch_team_issues`,
  `fetch_cycle`, `fetch_label`, `fetch_workflow_state`, `fetch_issue`,
  `fetch_project_issues`, `fetch_cycle_issues`, `fetch_label_issues`,
  `fetch_workflow_state_issues`, `fetch_issue_comments`, `fetch_comment`,
  `fetch_filtered_issues`.
- Helper `sorted_by_key`.

Delete: every mutation fn (`plan_mutations`, `execute`,
`execute_issue_*`, `execute_comment_create`, `create_issue`,
`update_issue`, `create_comment`, `append_optional_issue_fields`), the
`fetch_resource` / `list_scope` functions, every `*_files` projection
helper (`team_files`, `project_files`, ...), every `*_path` fn, every
`file_entry`/`resource_marker`, and the identity invalidation helpers
(`invalidate_issue_graph`, `invalidate_prefix`). Draft parsing and
version-check helpers (`parse_issue_draft`, `parse_comment_draft`,
`split_commit_path`, `parse_*_draft_source`, `parse_optional_*`,
`change_text`, `required_change_text`, `parse_scoped_uuid`,
`split_segments`, `normalize_path`, `format_number`) all go with
mutations.

Replace the old `graphql<T>` call-site:

```rust
// old
async fn graphql<T>(cx: &Cx<State>, query: String, variables: Value) -> ProviderResult<T>
where T: DeserializeOwned
{
    let body = serde_json::to_vec(&json!({ "query": query, "variables": variables }))?;
    let response_bytes = cx.http().post_json_bytes(GRAPHQL_ENDPOINT, body).await?;
    // ... decode envelope ...
}
```

with:

```rust
use omnifs_sdk::Cx;
use omnifs_sdk::prelude::*;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use crate::http_ext::LinearHttpExt;
use crate::{Result, State};

#[derive(Debug, Serialize)]
struct GraphQlRequest<'a> {
    query: &'a str,
    variables: &'a Value,
}

pub(crate) async fn graphql<T>(
    cx: &Cx<State>,
    query: &str,
    variables: Value,
) -> Result<T>
where
    T: DeserializeOwned,
{
    let response_bytes = cx
        .linear_graphql()
        .json(&GraphQlRequest { query, variables: &variables })
        .send_body()
        .await?;
    let envelope: GraphQlEnvelope<T> = serde_json::from_slice(&response_bytes)
        .map_err(|e| ProviderError::internal(format!("decode Linear GraphQL response: {e}")))?;
    if !envelope.errors.is_empty() {
        let message = envelope.errors.iter()
            .map(|e| e.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        // Rate-limit detection (best-effort; no rate_limited helper on
        // the new SDK, see Risks): surface as invalid_input with a
        // recognisable message prefix so operators can grep logs.
        let is_rl = envelope.errors.iter().any(|e| {
            matches!(e.extensions.code.as_deref(), Some("RATELIMITED" | "RATE_LIMITED"))
        });
        return Err(ProviderError::invalid_input(if is_rl {
            format!("linear rate limited: {message}")
        } else {
            message
        }));
    }
    envelope.data.ok_or_else(|| ProviderError::internal("Linear GraphQL response missing data"))
}

fn sorted_by_key<T, F>(mut items: Vec<T>, key: F) -> Vec<T>
where F: Fn(&T) -> String,
{
    items.sort_by_key(key);
    items
}
```

The individual `fetch_*` fns change only in signature (`ProviderResult`
to `Result`) and in how they call `graphql`:

```rust
// before
let data: TeamsData = graphql(cx, query, json!({ "first": cx.state(|s| s.config.page_size) })).await?;

// after (identical, just the Result alias differs)
let data: TeamsData = graphql(cx, &query, json!({ "first": cx.state(|s| s.config.page_size) })).await?;
```

The `ISSUE_FIELDS` constant and other `*_FIELDS` strings are GraphQL
fragments. Keep them verbatim.

### `src/events.rs` — rewrite (mirroring `providers/github/src/events.rs`)

Old content is a single `DEFAULT_REFRESH_SECS` constant. New content is
the full timer-tick polling loop, adapted from the GitHub reference:

```rust
use omnifs_sdk::Cx;
use omnifs_sdk::omnifs::provider::types::HttpResponse;
use omnifs_sdk::prelude::*;
use serde::Deserialize;
use serde_json::json;

use crate::graphql::IssueRecord;
use crate::http_ext::LinearHttpExt;
use crate::types::TeamKey;
use crate::{Result, State};

#[derive(Debug, Deserialize)]
struct RecentIssuesEnvelope {
    data: Option<RecentIssuesData>,
}

#[derive(Debug, Deserialize)]
struct RecentIssuesData {
    issues: RecentIssuesConnection,
}

#[derive(Debug, Deserialize)]
struct RecentIssuesConnection {
    nodes: Vec<RecentIssueNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecentIssueNode {
    identifier: String,
    team: RecentIssueTeam,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct RecentIssueTeam {
    key: String,
}

struct TickOutcome {
    team_key: TeamKey,
    response: Result<HttpResponse>,
}

pub(crate) async fn timer_tick(cx: Cx<State>) -> Result<EventOutcome> {
    let mut outcome = EventOutcome::new();

    // Collect every distinct team_key visible in any active mount path
    // that captures `{team}` in the first segment. We cover both the
    // `/{team}/...` family (TeamHandlers::team) and the
    // `/{team}/issues/...` tree by asking for every live path and
    // extracting the first segment.
    let mut team_keys = cx.active_paths("/{team}", |p| first_segment_as_team(p));
    for mount_id in [
        "/{team}/issues",
        "/{team}/issues/{issue}",
        "/{team}/cycles",
        "/{team}/labels",
        "/{team}/workflow_states",
    ] {
        team_keys.extend(cx.active_paths(mount_id, |p| first_segment_as_team(p)));
    }
    team_keys.sort_by_key(|k| k.as_ref().to_string());
    team_keys.dedup_by(|a, b| a.as_ref() == b.as_ref());

    if team_keys.is_empty() {
        return Ok(outcome);
    }

    let page_size = cx.state(|s| s.config.page_size.min(50));

    let fetches = team_keys.into_iter().map(|team_key| {
        let cx = cx.clone();
        let etag = cx.state(|s| s.team_issue_etags.get(team_key.as_ref()).cloned());
        async move {
            let query = format!(
                "query RecentIssues($teamKey: String!, $first: Int!) \
                 {{ issues(first: $first, orderBy: updatedAt, \
                    filter: {{ team: {{ key: {{ eq: $teamKey }} }} }}) \
                  {{ nodes {{ identifier team {{ key }} updatedAt }} }} }}"
            );
            let mut req = cx.linear_graphql()
                .json(&json!({
                    "query": query,
                    "variables": {
                        "teamKey": team_key.as_ref(),
                        "first": page_size,
                    },
                }));
            if let Some(etag) = etag {
                req = req.header("If-None-Match", etag);
            }
            let response = req.send().await;
            TickOutcome { team_key, response }
        }
    });
    let outcomes = join_all(fetches).await;

    let mut etag_updates: Vec<(String, String)> = Vec::new();
    let mut invalidations = hashbrown::HashSet::<String>::new();
    for tick in outcomes {
        let Ok(response) = tick.response else { continue };
        if response.status == 304 || response.status >= 400 {
            continue;
        }
        if let Some(etag) = response.headers.iter()
            .find(|h| h.name.eq_ignore_ascii_case("etag"))
            .map(|h| h.value.clone())
        {
            etag_updates.push((tick.team_key.as_ref().to_string(), etag));
        }
        // The Linear API may not return etags on GraphQL POSTs at all.
        // In that case we fall back to hash-of-body to detect change.
        // See Risks: "Linear GraphQL etag support".
        let Ok(envelope) = serde_json::from_slice::<RecentIssuesEnvelope>(&response.body) else {
            continue;
        };
        let Some(data) = envelope.data else { continue };
        if data.issues.nodes.is_empty() {
            continue;
        }

        // Any update for this team invalidates every prefix that listed
        // its issues. The prefix set mirrors the old
        // invalidate_issue_graph but with path-only invalidations.
        let team = tick.team_key.as_ref();
        invalidations.insert(format!("/{team}"));
        invalidations.insert(format!("/{team}/issues"));
        invalidations.insert(format!("/{team}/cycles"));
        invalidations.insert(format!("/{team}/labels"));
        invalidations.insert(format!("/{team}/workflow_states"));
        invalidations.insert(format!("/{team}/projects"));
        // Project listing is global too: any change to any team issue
        // can drift the cross-team `/_projects/*/issues` lists.
        invalidations.insert("/_projects".to_string());
    }

    if !etag_updates.is_empty() {
        cx.state_mut(|state| {
            for (team, etag) in etag_updates.drain(..) {
                state.team_issue_etags.insert(team, etag);
            }
        });
    }
    for prefix in invalidations {
        outcome.invalidate_prefix(prefix);
    }
    Ok(outcome)
}

fn first_segment_as_team(absolute_path: &str) -> Option<TeamKey> {
    let trimmed = absolute_path.trim_start_matches('/');
    let first = trimmed.split('/').next()?;
    first.parse::<TeamKey>().ok()
}
```

### `src/root.rs` — rewrite

Replace the `Dir` trait impl with a `#[handlers]` block. The root shows
the three well-known containers (`_projects`, plus dynamic team dirs)
and enumerates fetched teams when available:

```rust
use omnifs_sdk::prelude::*;

use crate::graphql;
use crate::{Result, State};

pub struct RootHandlers;

#[handlers]
impl RootHandlers {
    #[dir("/")]
    async fn root(cx: &DirCx<'_, State>) -> Result<Projection> {
        let mut projection = Projection::new();
        projection.dir("_projects");
        // Teams are enumerable via the API; project them so `ls /`
        // shows known teams alongside the static _projects dir.
        let teams = graphql::fetch_teams(cx).await?;
        for team in teams {
            projection.dir(team.key.to_string());
        }
        projection.page(PageStatus::Exhaustive);
        Ok(projection)
    }
}
```

### `src/team.rs` — new (replaces old `entities/team.rs` +
subsets of `entities/project.rs` + `entities/issue.rs`)

All the `/{team}`, `/{team}/issues`, `/{team}/projects`,
`/{team}/projects/{project}` handlers move here. Team dir lookups
project the team's sibling files (id, key, name, description.md) so
subsequent reads are served from cache:

```rust
use omnifs_sdk::prelude::*;

use crate::graphql::{self, IssueRecord, ProjectRecord, TeamRecord};
use crate::types::{IssueKey, TeamKey, UuidKey};
use crate::{Result, State};

pub struct TeamHandlers;

#[handlers]
impl TeamHandlers {
    #[dir("/{team}")]
    async fn team(cx: &DirCx<'_, State>, team: TeamKey) -> Result<Projection> {
        let record = graphql::fetch_team(cx, &team).await?;
        let mut p = Projection::new();
        p.file_with_content("id", record.id.to_string());
        p.file_with_content("key", record.key.to_string());
        p.file_with_content("name", record.name.clone());
        p.file_with_content(
            "description.md",
            record.description.clone().unwrap_or_default(),
        );
        p.dir("issues");
        p.dir("projects");
        p.dir("cycles");
        p.dir("labels");
        p.dir("workflow_states");
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/{team}/issues")]
    async fn team_issues(cx: &DirCx<'_, State>, team: TeamKey) -> Result<Projection> {
        let issues = graphql::fetch_team_issues(cx, &team).await?;
        let mut p = Projection::new();
        for issue in &issues {
            let base = format!("{}/{}", team, issue.identifier);
            preload_issue_fields(&mut p, &base, issue);
            p.dir(issue.identifier.to_string());
        }
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/{team}/projects")]
    async fn team_projects(cx: &DirCx<'_, State>, team: TeamKey) -> Result<Projection> {
        let projects = graphql::fetch_team_projects(cx, &team).await?;
        let mut p = Projection::new();
        for project in projects {
            p.dir(project.id.to_string());
        }
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/{team}/projects/{project}")]
    async fn team_project_alias(
        cx: &DirCx<'_, State>,
        _team: TeamKey,
        project: UuidKey,
    ) -> Result<Projection> {
        let record = graphql::fetch_project(cx, &project).await?;
        let mut p = Projection::new();
        project_fields(&mut p, &record);
        p.dir("issues");
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }
}

fn project_fields(p: &mut Projection, record: &ProjectRecord) {
    p.file_with_content("id", record.id.to_string());
    p.file_with_content("name", record.name.clone());
    p.file_with_content("description.md", record.description.clone().unwrap_or_default());
    p.file_with_content("state", record.state.clone().unwrap_or_default());
    p.file_with_content("progress", record.progress.map_or_else(String::new, format_number));
    p.file_with_content("start_date", record.start_date.clone().unwrap_or_default());
    p.file_with_content("target_date", record.target_date.clone().unwrap_or_default());
}

pub(crate) fn preload_issue_fields(p: &mut Projection, base: &str, issue: &IssueRecord) {
    // All fields eagerly preloaded since the IssueRecord carries them.
    // Paths are relative to the listed directory (the team's issues dir).
    p.preload(format!("{base}/id"), issue.id.to_string());
    p.preload(format!("{base}/identifier"), issue.identifier.to_string());
    p.preload(format!("{base}/title"), issue.title.clone());
    p.preload(format!("{base}/description.md"),
              issue.description.clone().unwrap_or_default());
    p.preload(format!("{base}/priority"),
              issue.priority.map_or_else(String::new, format_number));
    p.preload(format!("{base}/estimate"),
              issue.estimate.map_or_else(String::new, format_number));
    p.preload(format!("{base}/created_at"), issue.created_at.clone());
    p.preload(format!("{base}/updated_at"), issue.updated_at.clone());
    p.preload(
        format!("{base}/assignee"),
        issue.assignee.as_ref().map_or_else(String::new, |u| u.name.clone()),
    );
}

pub(crate) fn format_number(value: f64) -> String {
    if value.fract() == 0.0 { format!("{value:.0}") } else { value.to_string() }
}
```

### `src/issue.rs` — new (replaces `entities/issue.rs` +
`entities/comment.rs`)

```rust
use omnifs_sdk::prelude::*;

use crate::graphql::{self, CommentRecord, IssueRecord};
use crate::team::{format_number, preload_issue_fields};
use crate::types::{IssueKey, TeamKey, UuidKey};
use crate::{Result, State};

pub struct IssueHandlers;

#[handlers]
impl IssueHandlers {
    #[dir("/{team}/issues/{issue}")]
    async fn issue(
        cx: &DirCx<'_, State>,
        _team: TeamKey,
        issue: IssueKey,
    ) -> Result<Projection> {
        let record = graphql::fetch_issue(cx, &issue).await?;
        let mut p = Projection::new();
        issue_fields(&mut p, &record);
        // The issue payload already carries its comments; stage them.
        for comment in &record.comments.nodes {
            let base = format!("comments/{}", comment.id);
            preload_comment_fields(&mut p, &base, comment);
        }
        p.dir("comments");
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/{team}/issues/{issue}/comments")]
    async fn issue_comments(
        cx: &DirCx<'_, State>,
        _team: TeamKey,
        issue: IssueKey,
    ) -> Result<Projection> {
        let comments = graphql::fetch_issue_comments(cx, &issue).await?;
        let mut p = Projection::new();
        for comment in &comments {
            let base = comment.id.to_string();
            preload_comment_fields(&mut p, &base, comment);
            p.dir(comment.id.to_string());
        }
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/{team}/issues/{issue}/comments/{comment}")]
    async fn issue_comment(
        cx: &DirCx<'_, State>,
        _team: TeamKey,
        issue: IssueKey,
        comment: UuidKey,
    ) -> Result<Projection> {
        let record = graphql::fetch_comment(cx, &issue, &comment).await?;
        let mut p = Projection::new();
        comment_fields(&mut p, &record);
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }
}

fn issue_fields(p: &mut Projection, issue: &IssueRecord) {
    p.file_with_content("id", issue.id.to_string());
    p.file_with_content("identifier", issue.identifier.to_string());
    p.file_with_content("title", issue.title.clone());
    p.file_with_content("description.md", issue.description.clone().unwrap_or_default());
    p.file_with_content("priority",
        issue.priority.map_or_else(String::new, format_number));
    p.file_with_content("estimate",
        issue.estimate.map_or_else(String::new, format_number));
    p.file_with_content("created_at", issue.created_at.clone());
    p.file_with_content("updated_at", issue.updated_at.clone());
    p.file_with_content(
        "assignee",
        issue.assignee.as_ref().map_or_else(String::new, |u| u.name.clone()),
    );
}

fn comment_fields(p: &mut Projection, comment: &CommentRecord) {
    p.file_with_content("id", comment.id.to_string());
    p.file_with_content(
        "author",
        comment.user.as_ref().map_or_else(String::new, |u| u.name.clone()),
    );
    p.file_with_content("created_at", comment.created_at.clone());
    p.file_with_content("updated_at", comment.updated_at.clone());
    p.file_with_content("edited", if comment.edited { "true" } else { "false" });
    p.file_with_content("body.md", comment.body.clone().unwrap_or_default());
}

fn preload_comment_fields(p: &mut Projection, base: &str, comment: &CommentRecord) {
    p.preload(format!("{base}/id"), comment.id.to_string());
    p.preload(
        format!("{base}/author"),
        comment.user.as_ref().map_or_else(String::new, |u| u.name.clone()),
    );
    p.preload(format!("{base}/created_at"), comment.created_at.clone());
    p.preload(format!("{base}/updated_at"), comment.updated_at.clone());
    p.preload(format!("{base}/edited"),
        if comment.edited { "true" } else { "false" }.to_string());
    p.preload(format!("{base}/body.md"), comment.body.clone().unwrap_or_default());
}
```

### `src/project.rs` — new (replaces `entities/project.rs`)

Hosts `/_projects`, `/_projects/{project}`,
`/_projects/{project}/issues`, `/_projects/{project}/issues/{issue}`:

```rust
use omnifs_sdk::prelude::*;

use crate::graphql::{self, IssueRecord, ProjectRecord};
use crate::issue;
use crate::team::{format_number, preload_issue_fields};
use crate::types::{IssueKey, UuidKey};
use crate::{Result, State};

pub struct ProjectHandlers;

#[handlers]
impl ProjectHandlers {
    #[dir("/_projects")]
    async fn all_projects(cx: &DirCx<'_, State>) -> Result<Projection> {
        let projects = graphql::fetch_projects(cx).await?;
        let mut p = Projection::new();
        for project in projects {
            p.dir(project.id.to_string());
        }
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/_projects/{project}")]
    async fn project(cx: &DirCx<'_, State>, project: UuidKey) -> Result<Projection> {
        let record = graphql::fetch_project(cx, &project).await?;
        let mut p = Projection::new();
        p.file_with_content("id", record.id.to_string());
        p.file_with_content("name", record.name.clone());
        p.file_with_content("description.md", record.description.clone().unwrap_or_default());
        p.file_with_content("state", record.state.clone().unwrap_or_default());
        p.file_with_content("progress", record.progress.map_or_else(String::new, format_number));
        p.file_with_content("start_date", record.start_date.clone().unwrap_or_default());
        p.file_with_content("target_date", record.target_date.clone().unwrap_or_default());
        p.dir("issues");
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/_projects/{project}/issues")]
    async fn project_issues(cx: &DirCx<'_, State>, project: UuidKey) -> Result<Projection> {
        let issues = graphql::fetch_project_issues(cx, &project).await?;
        let mut p = Projection::new();
        for issue in &issues {
            let base = issue.identifier.to_string();
            preload_issue_fields(&mut p, &base, issue);
            p.dir(issue.identifier.to_string());
        }
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/_projects/{project}/issues/{issue}")]
    async fn project_issue(
        cx: &DirCx<'_, State>,
        _project: UuidKey,
        issue: IssueKey,
    ) -> Result<Projection> {
        // Delegate to the main issue projection: same shape.
        issue::issue_alias(cx, issue).await
    }
}
```

Add a helper `issue::issue_alias` in `issue.rs` that runs the same
projection as `IssueHandlers::issue` but without the team capture (the
payload carries the team). Move the `issue_fields` body into it and
call it from both places.

### `src/cycle.rs` — new (replaces `entities/cycle.rs`)

`/{team}/cycles`, `/{team}/cycles/{cycle}`,
`/{team}/cycles/{cycle}/issues`, `/{team}/cycles/{cycle}/issues/{issue}`.

```rust
use omnifs_sdk::prelude::*;

use crate::graphql::{self, CycleRecord};
use crate::issue;
use crate::team::{format_number, preload_issue_fields};
use crate::types::{IssueKey, TeamKey, UuidKey};
use crate::{Result, State};

pub struct CycleHandlers;

#[handlers]
impl CycleHandlers {
    #[dir("/{team}/cycles")]
    async fn cycles(cx: &DirCx<'_, State>, team: TeamKey) -> Result<Projection> {
        let cycles = graphql::fetch_team_cycles(cx, &team).await?;
        let mut p = Projection::new();
        for cycle in cycles {
            p.dir(cycle.id.to_string());
        }
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/{team}/cycles/{cycle}")]
    async fn cycle(
        cx: &DirCx<'_, State>,
        _team: TeamKey,
        cycle: UuidKey,
    ) -> Result<Projection> {
        let record = graphql::fetch_cycle(cx, &cycle).await?;
        let mut p = Projection::new();
        cycle_fields(&mut p, &record);
        p.dir("issues");
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/{team}/cycles/{cycle}/issues")]
    async fn cycle_issues(
        cx: &DirCx<'_, State>,
        _team: TeamKey,
        cycle: UuidKey,
    ) -> Result<Projection> {
        let issues = graphql::fetch_cycle_issues(cx, &cycle).await?;
        let mut p = Projection::new();
        for issue in &issues {
            let base = issue.identifier.to_string();
            preload_issue_fields(&mut p, &base, issue);
            p.dir(issue.identifier.to_string());
        }
        p.page(PageStatus::Exhaustive);
        Ok(p)
    }

    #[dir("/{team}/cycles/{cycle}/issues/{issue}")]
    async fn cycle_issue(
        cx: &DirCx<'_, State>,
        _team: TeamKey,
        _cycle: UuidKey,
        issue: IssueKey,
    ) -> Result<Projection> {
        issue::issue_alias(cx, issue).await
    }
}

fn cycle_fields(p: &mut Projection, cycle: &CycleRecord) {
    p.file_with_content("id", cycle.id.to_string());
    p.file_with_content("name", cycle.name.clone());
    p.file_with_content("description.md", cycle.description.clone().unwrap_or_default());
    p.file_with_content("starts_at", cycle.starts_at.clone().unwrap_or_default());
    p.file_with_content("ends_at", cycle.ends_at.clone().unwrap_or_default());
    p.file_with_content("progress", cycle.progress.map_or_else(String::new, format_number));
}
```

### `src/label.rs` — new (replaces `Label` parts of
`entities/workflow_state.rs`)

`/{team}/labels`, `/{team}/labels/{label}`,
`/{team}/labels/{label}/issues`, `/{team}/labels/{label}/issues/{issue}`.
Same shape as cycles; swap `cycle_fields` for `label_fields`:

```rust
fn label_fields(p: &mut Projection, label: &LabelRecord) {
    p.file_with_content("id", label.id.to_string());
    p.file_with_content("name", label.name.clone());
    p.file_with_content("color", label.color.clone().unwrap_or_default());
    p.file_with_content("description.md", label.description.clone().unwrap_or_default());
}
```

### `src/workflow_state.rs` — new (replaces `WorkflowState`
parts of `entities/workflow_state.rs`)

`/{team}/workflow_states`, `/{team}/workflow_states/{state}`,
`/{team}/workflow_states/{state}/issues`,
`/{team}/workflow_states/{state}/issues/{issue}`. Same shape, with:

```rust
fn state_fields(p: &mut Projection, state: &WorkflowStateRecord) {
    p.file_with_content("id", state.id.to_string());
    p.file_with_content("name", state.name.clone());
    p.file_with_content("type", state.kind.clone());
    p.file_with_content("position",
        state.position.map_or_else(String::new, format_number));
    p.file_with_content("color", state.color.clone().unwrap_or_default());
    p.file_with_content("description.md",
        state.description.clone().unwrap_or_default());
}
```

### `src/entities/` — delete entire directory

All modules (`mod.rs`, `comment.rs`, `cycle.rs`, `draft.rs`, `issue.rs`,
`project.rs`, `root.rs`, `team.rs`, `workflow_state.rs`) are replaced
by the top-level per-family files above.

### `src/api.rs` — delete

All of `api.rs` is either moved to `graphql.rs` (fetches) or dropped
(mutations/scope/identity).

---

## Cargo.toml changes

### Provider (`providers/linear/Cargo.toml`)

Add `hashbrown` and `serde_json` as direct deps to match
`providers/github/Cargo.toml`:

```toml
[dependencies]
omnifs-sdk = { path = "../../crates/omnifs-sdk" }
hashbrown = "0.15"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Everything else in the Cargo.toml stays (lints, crate-type, metadata).

### Workspace root (`Cargo.toml`)

After merging main, the root `members` list reads:

```toml
members = ["crates/*", "providers/github", "providers/dns", "providers/test"]
```

Append `"providers/linear"`:

```toml
members = ["crates/*", "providers/github", "providers/dns", "providers/test", "providers/linear"]
```

Do not change `default-members`; providers always build separately with
`--target wasm32-wasip2`.

---

## Verification

Run in order from `/Users/raul/W/gvfs/.worktrees/providers/linear`:

```bash
# 1. Formatting check (fails CI otherwise).
cargo fmt --check

# 2. Host crates compile (sanity after merge).
cargo check -p omnifs-host

# 3. Provider lint with denied warnings.
cargo clippy -p omnifs-provider-linear --target wasm32-wasip2 -- -D warnings

# 4. Provider tests compile (cannot run on wasm32-wasip2 without a
#    runtime in the test harness).
cargo test -p omnifs-provider-linear --target wasm32-wasip2 --no-run

# 5. Full provider matrix (github + dns + linear).
just check-providers

# 6. (Optional) Smoke test under docker, if integration mounts exist.
just dev
```

Expected outcomes:

- `cargo fmt --check`: clean.
- `cargo clippy ... -- -D warnings`: no warnings. Any pedantic
  warnings coming from the new code must be silenced by minimal refactor
  (not blanket `allow`s).
- `cargo test ... --no-run`: every target-specific test builds.
- `just check-providers`: passes across github, dns, linear.

---

## Risks and gotchas

### 1. SDK HTTP builder is GET-only on main

Already covered in "Required SDK additions". Without adding `post` /
`body` / `json` to `http::Builder` + `Request`, nothing in the linear
provider compiles. This is a small, structural change (not a feature),
and scopes cleanly to `crates/omnifs-sdk/src/http.rs`.

### 2. No `rate_limited`, `permission_denied`, `version_mismatch`
convenience constructors

The old code used `ProviderError::rate_limited(...)`,
`ProviderError::permission_denied(...)`,
`ProviderError::version_mismatch(...)`. The new SDK's prelude exposes
`not_found`, `invalid_input`, `not_a_directory`, `not_a_file`,
`internal`, `unimplemented`. Rate-limit errors now surface as
`invalid_input` with a `linear rate limited:` prefix (see the updated
`graphql()` helper above). Version-mismatch is dead with mutations; no
call site remains. Permission-denied is likewise only used in the
deleted mutations pipeline.

If operators want typed rate-limit handling, that is a separate SDK
extension: add `ProviderErrorKind::RateLimited` helpers in
`crates/omnifs-sdk/src/error.rs`. Out of scope here unless the test
matrix exposes a specific gap.

### 3. Linear GraphQL may not return HTTP etags on POST

The GitHub event loop uses etags because GitHub's `/events` endpoint
returns them. Linear's GraphQL endpoint is POST-only and typically does
not set `ETag`. Treat the etag path as best-effort in `on_event`: when
absent, the loop always invalidates the configured prefixes on each
tick. Accept the conservative invalidation; the host owns caching and
re-fetches are cheap relative to UI correctness.

Do not reintroduce provider-side deduping or TTLs to "smooth" this.

### 4. GraphQL query string construction is dynamic

Several queries in `api.rs` build GraphQL via `format!("... {ISSUE_FIELDS}
... filter: {{ team: {{ key: {{ eq: \"{}\" }} }} }}", team_key)`. That
interpolation is bounded by the validators on `TeamKey` / `IssueKey` /
`UuidKey` (ascii alphanumeric + a narrow allow list) — GraphQL injection
is blocked by the type system at the public surface. Keep the validators
strict; don't relax `is_safe_segment` to, say, accept spaces.

### 5. `enable_mutations` is gone

The old config exposed `enable_mutations: bool` to gate `execute()`.
That field disappears with the mutation pipeline. Producing a config
document with `"enable_mutations": true` now silently serde-rejects (no
such field) unless `#[serde(deny_unknown_fields)]` is set (it isn't).
Silent is acceptable for deferred functionality; if operators complain,
add a warning via a `validate_config` step later.

### 6. Pagination cursors: no pagination in current fetches

The old `api.rs` never paged any listing; it uses `first: $first` once
with `config.page_size` and accepts the cap. Keep this behaviour. The
new SDK supports cursored pagination via
`PageStatus::More(Cursor::Opaque(..))`, but rewiring every listing to
paginate is out of scope; mark every completing projection with
`PageStatus::Exhaustive`. Mark non-exhaustive only if/when pagination
is added. Document "known cap of `config.page_size`" near each fetch
if the team cares about visible truncation.

### 7. Webhook verification

Linear supports webhook signing (HMAC SHA-256 with the
`LINEAR_WEBHOOK_SIGNATURE` header). The old code did not verify
webhooks. The new WIT delivers `ProviderEvent::WebhookReceived(bytes)`
but the raw bytes lose the transport headers (the WIT variant is
`webhook-received(list<u8>)`, no headers). Verifying the signature
currently requires the host to forward headers through a richer event
shape, which is an out-of-scope WIT change. Leave webhook handling as a
no-op arm of `on_event` and document that signature verification is not
yet possible.

### 8. Issue description and comment body are Markdown

`description.md` and `body.md` land as sibling files via
`file_with_content`. Sizes can exceed the 64 KiB eager cap. When they
do, the `Projection::file_with_content` call sets
`projection.error = Some("projected file exceeds eager byte limit")`,
which surfaces as `ProviderError::invalid_input` at dispatch. To avoid
that failure mode, clamp: if a description is over 64 KiB, stage the
over-limit paths via `preload` (which accepts arbitrary sizes) and
register just the name + stat via `file_with_stat`:

```rust
const MAX_EAGER: usize = 64 * 1024;
if description.len() <= MAX_EAGER {
    p.file_with_content("description.md", description);
} else {
    let size = NonZeroU64::new(description.len() as u64).unwrap();
    p.file_with_stat("description.md", FileStat { size });
    p.preload("description.md", description); // path relative to dir
}
```

Apply the same clamp to comment `body.md`. This preserves the cached-read
optimisation for normal content and correctly falls back for edge cases.

### 9. Comment thread shape

Comments are a dir per comment with six sibling files. The old code
called `fetch_issue` from `Comments::load` and surfaced comments as
`items`. The new approach keeps `fetch_issue_comments` but also uses
the existing `Issue::{comments{nodes}}` connection already fetched by
`IssueHandlers::issue` to preload children, saving a round-trip when
the user first lands in `/{team}/issues/{id}/`. `IssueHandlers::issue`
handles the preload; `IssueHandlers::issue_comments` handles standalone
entry (e.g. deep-linked `cd /FIB/issues/FIB-1/comments/`).

### 10. `hashbrown` on the State struct

The old `State` was just `{ config: Config }`. The new one adds
`team_issue_etags: hashbrown::HashMap<String, String>`. Match the
github provider's pattern (`event_etags: hashbrown::HashMap<RepoId,
String>`) — wasm predictability per CLAUDE.md. Using
`std::collections::HashMap` here would work but drifts from the
documented convention.

---

## Verification

- `cargo fmt --check`
- `cargo clippy -p omnifs-provider-linear --target wasm32-wasip2 -- -D warnings`
- `cargo test -p omnifs-provider-linear --target wasm32-wasip2 --no-run`
- `just check-providers`

All must pass. If `just` is not on PATH, note that in the PR body and run
the equivalent `cargo` commands from the root of this branch's worktree.

## Commit

Conventional:

```
feat(linear): migrate provider to path-first handler SDK
```

Body: one paragraph naming the major structural changes and the base SDK
PRs (#28 `feat/sdk-http-post-support`, #29 `feat/sdk-path-rest-captures`,
#D `feat/sdk-error-constructors`).

## PR

- Title: `feat(linear): migrate provider to path-first handler SDK`
- Body: summary + link to this branch's `PLAN.md` + verification results
  (which cargo commands ran, which passed, which were skipped and why).
