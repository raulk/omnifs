# Protocol shape: callouts, terminals, and folded op-results

Status: draft, ready to implement
Scope: `wit/provider.wit`, host runtime, SDK, SDK macros, all three providers, tests
Branch: open TBD

## Goal

Tighten the provider protocol vocabulary so that three distinct semantic channels show up as three distinct things in the WIT: callouts (work the host runs and reports back), terminals (the handler's typed answer to a FUSE operation, carrying any cache effects it implies), and sidecar dispatches like `materialize` disappear into their parent operation's terminal. As a side effect, cut dead variants and rename types that were named from the host's implementation rather than the provider's contract.

## Current state

After the effect-boundary refactor (now landed on `feat/provider-sdk-dx-redesign`):

```wit
variant effect {
    fetch(http-request),
    stream-open / stream-recv / stream-close,
    ws-connect / ws-send / ws-recv / ws-close,
    git-open-repo / git-list-tree / git-read-blob / git-head-ref / git-list-cached-repos,
    preload-paths(list<preloaded-file>),
    invalidate-path(string),
    invalidate-prefix(string),
}

variant effect-result {
    http-response, stream-opened, stream-chunk, ws-*, git-*,
    ack,
    effect-error(effect-error),
}

variant action-result {
    dir-entries(dir-listing),
    lookup(lookup-result),
    file(file-content-result),
    not-materialized,
    disowned-tree(tree-ref),
    ok,
    provider-err(provider-error),
    provider-initialized(provider-info),
    // reserved mutation variants
}

record provider-response {
    effects: list<effect>,
    terminal: option<action-result>,
}

interface browse {
    lookup-child, list-children, read-file, materialize, ...
}
```

Three problems:

1. `effect` conflates two semantically different channels: callouts expect replies, fire-and-forget effects mutate host state. `effect-result::ack` is padding for the second kind. The SDK already distinguishes (`cx.preload_paths` is sync; `cx.http().get(...).send()` returns a future), but the WIT doesn't.
2. `action-result` has variants that are structurally operation-specific (`not-materialized` and `disowned-tree` belong to `materialize`; `provider-initialized` belongs to `initialize`; `ok` is the degenerate return of `on-event`). They sit at the top level as siblings of real terminals.
3. `materialize` is a separate browse method whose sole job is to answer "is this path a subtree handoff?" The host calls it at the top of `lookup-child` / `list-children` / `opendir` anyway, so the sidecar method adds a dispatch step and two variant arms for no structural gain.

## Target WIT

```wit
/// Work the host runs on the provider's behalf. Every callout expects
/// a matching `callout-result` back via `resume`; there are no
/// fire-and-forget callouts.
variant callout {
    fetch(http-request),
    stream-open(http-request),
    stream-recv(stream-id),
    stream-close(stream-id),
    ws-connect(ws-connect-request),
    ws-send(ws-send-request),
    ws-recv(ws-recv-request),
    ws-close(stream-id),
    git-open-repo(git-open-request),
}

variant callout-result {
    http-response(http-response),
    stream-opened(stream-id),
    stream-chunk(option<list<u8>>),
    stream-closed,
    ws-connected(stream-id),
    ws-message(option<list<u8>>),
    ws-closed,
    git-repo-opened(git-repo-info),
    callout-error(callout-error),
}

type callout-results = list<callout-result>;

/// Content the provider hands the host so a later read of each path is
/// served without another provider round trip. Emitted as a field of
/// `dir-listing`, not as a separate effect.
record preloaded-file {
    path: string,
    content: list<u8>,
}

/// Directory listing terminal, emitted by `list-children` and as the
/// underlying dir-entry content of a `lookup-child` against a directory.
record dir-listing {
    entries: list<dir-entry>,
    exhaustive: bool,
    preload: list<preloaded-file>,
}

/// Event handler terminal. Carries invalidations the host must apply at
/// the response boundary.
record event-outcome {
    invalidate-paths: list<string>,
    invalidate-prefixes: list<string>,
}

/// Lookup terminal. `subtree(tree-ref)` replaces the old
/// `disowned-tree` variant and lets `lookup-child` return a subtree
/// handoff directly.
variant lookup-result {
    entry(lookup-entry),
    subtree(tree-ref),
    not-found,
}

/// The non-subtree, non-miss shape of a lookup: the found entry plus
/// cache-adjacent data. `target` is non-optional because the miss
/// case is the `not-found` arm of `lookup-result`, not a null target.
record lookup-entry {
    target: dir-entry,
    siblings: list<dir-entry>,
    sibling-files: list<projected-file>,
    exhaustive: bool,
}

/// List terminal. Same pattern.
variant list-result {
    entries(dir-listing),
    subtree(tree-ref),
}

/// Read terminal, unchanged from today beyond the rename path. Keeps
/// the `sibling-files` field: the per-operation equivalent of
/// `dir-listing.preload`, carrying content for paths adjacent to the
/// file being read.
record file-content-result {
    content: list<u8>,
    sibling-files: list<projected-file>,
}

variant op-result {
    lookup(lookup-result),
    list(list-result),
    read(file-content-result),
    init(provider-info),
    event(event-outcome),
    err(provider-error),
    // Reserved for the mutation protocol. Kept in the same shape as
    // the browse terminals: variant arm name matches the operation
    // name, not the shape of the payload. Providers do not implement
    // these today; the macro returns `err(provider-error::unimplemented)`.
    plan-mutations(list<planned-mutation>),
    execute(mutation-outcome),
    fetch-resource(list<file-entry>),
}

record provider-return {
    callouts: list<callout>,
    terminal: option<op-result>,
}

interface browse {
    lookup-child: func(id, parent-path, name) -> provider-return;
    list-children: func(id, path) -> provider-return;
    read-file: func(id, path) -> provider-return;
    // materialize removed
}
```

The meaningful shape shifts in one place:

- `callout` is strictly request/response. Fire-and-forget is gone.
- `dir-listing` grows `preload` inline. `event-outcome` carries invalidations. Neither appears in `callout`.
- `action-result` becomes `op-result` and every non-`err` variant is 1:1 with a handler operation.
- `not-materialized` / `disowned-tree` fold into `lookup-result::not-found` / `{lookup,list}-result::subtree`.
- `materialize` method deletes. `#[subtree]` handlers dispatch from inside `lookup-child` and `list-children`.
- `ok` variant deletes: `on-event` handlers return `event(event-outcome { invalidate-paths: [], invalidate-prefixes: [] })` when there's nothing to invalidate.
- `ack` callout-result deletes: no variant needs positional padding anymore.
- Dead git callouts (`git-list-tree`, `git-read-blob`, `git-head-ref`, `git-list-cached-repos`) delete: no provider calls them end-to-end after path-first moved repo browsing to FUSE bind-mount.

## Rename table

| Old | New | Reason |
|---|---|---|
| `provider-response` | `provider-return` | "Return" reads as the handler's exit value; "response" was HTTP-brained. |
| `effect` | `callout` | Reflects request/response semantics after cache ops move out. |
| `effect-result` | `callout-result` | Mirror. |
| `effect-error` | `callout-error` | Mirror. |
| `effect-results` | `callout-results` | Mirror. |
| `action-result` | `op-result` | Every variant corresponds to a handler operation; "action" was vague. |
| `cache-preload` callout arm | `dir-listing.preload` field | Not a callout, not an action; it's content the listing carries. |
| `cache-invalidate-path` callout arm | `event-outcome.invalidate-paths` field | Ditto. |
| `cache-invalidate-prefix` callout arm | `event-outcome.invalidate-prefixes` field | Ditto. |
| `ack` callout-result arm | removed | No fire-and-forget in callouts. |
| `not-materialized` action-result arm | `lookup-result::not-found` | Fold into owner operation. |
| `disowned-tree(tree-ref)` action-result arm | `lookup-result::subtree`, `list-result::subtree` | Fold into owner operations. |
| `ok` action-result arm | removed; use `event(event-outcome{...})` | No longer needed. |
| `browse.materialize` method | removed | Folds into `lookup-child` and `list-children`. |
| `git-list-tree`, `git-read-blob`, `git-head-ref`, `git-list-cached-repos` callout arms | removed | Dead: no provider calls them. |
| `mutations-planned(...)` action-result arm | `plan-mutations(list<planned-mutation>)` | Arm name matches operation, per the fold rule. |
| `mutation-executed(mutation-outcome)` action-result arm | `execute(mutation-outcome)` | Arm name matches operation. |
| `resource-files(list<file-entry>)` action-result arm | `fetch-resource(list<file-entry>)` | Arm name matches operation. |
| `EffectRuntime` (Rust) | `CalloutRuntime` | Keep rename consistent with WIT. |
| `EffectFuture` (Rust) | `CalloutFuture` | Keep rename consistent with WIT. |
| `execute_single_effect` / `execute_batch` | `execute_single_callout` / `execute_batch` | Keep rename consistent. |
| `drive_effects` | `drive_callouts` | Keep rename consistent. |

## SDK impact

### `Cx` loses three methods

```rust
// Removed
impl<S> Cx<S> {
    pub fn preload_paths<I, P, B>(&self, files: I) { ... }
    pub fn invalidate_path(&self, path: impl Into<String>) { ... }
    pub fn invalidate_prefix(&self, prefix: impl Into<String>) { ... }
}
```

These moved to terminal builders.

### `Projection` gains preload

```rust
impl Projection {
    // Return () to match the style of `projection.file(...)` etc. Chaining
    // is not the prevailing pattern in Projection.
    pub fn preload(&mut self, path: impl Into<String>, content: impl Into<Vec<u8>>);
    pub fn preload_many<I, P, B>(&mut self, files: I)
    where
        I: IntoIterator<Item = (P, B)>,
        P: Into<String>,
        B: Into<Vec<u8>>;
}
```

Used by `#[dir]` handlers that were previously calling `cx.preload_paths(...)`. The `Projection` builder accumulates into the `preload` field of the eventual `dir-listing`. Empty paths and content rejected silently like the rest of the Projection API; duplicate paths are allowed (the host dedupes by path-key).

### New `EventOutcome` builder for `on-event`

```rust
#[derive(Default)]
pub struct EventOutcome {
    pub(crate) invalidate_paths: Vec<String>,
    pub(crate) invalidate_prefixes: Vec<String>,
}

impl EventOutcome {
    pub fn new() -> Self { Self::default() }

    pub fn invalidate_path(&mut self, path: impl Into<String>) -> &mut Self { ... }
    pub fn invalidate_prefix(&mut self, prefix: impl Into<String>) -> &mut Self { ... }
}
```

### `on-event` dispatch shape

The user-facing signature is `async fn on_event(cx: Cx<State>, event: ProviderEvent) -> Result<EventOutcome>`. No `ProviderReturn` construction in user code: the macro wraps the result.

The macro's generated `notify::Guest` impl becomes:

```rust
impl omnifs_sdk::exports::omnifs::provider::notify::Guest for #type_name {
    fn on_event(id: u64, event: ProviderEvent) -> ProviderReturn {
        let Ok(state) = state_handle() else {
            return omnifs_sdk::prelude::err(
                ProviderError::internal("provider not initialized")
            );
        };
        let cx = Cx::<#state_type>::from_event(id, state, &event);
        let future: Pin<Box<dyn Future<Output = ProviderReturn>>> = Box::pin({
            let cx = cx.clone();
            async move {
                // If the user provided on_event, call it; else return an
                // empty EventOutcome.
                #user_body_or_default
            }
        });
        ASYNC_RUNTIME.with(|runtime| runtime.start(id, cx, future))
    }

    fn cancel(id: u64) { ... }
}
```

Where `#user_body_or_default` expands to either:
```rust
// user-provided path
match #type_name::on_event(cx, event).await {
    Ok(outcome) => ProviderReturn::terminal(OpResult::Event(outcome.into_wit())),
    Err(error) => omnifs_sdk::prelude::err(error),
}
```
or (default, when the provider didn't define `on_event`):
```rust
let _ = (cx, event);
ProviderReturn::terminal(OpResult::Event(EventOutcome::new().into_wit()))
```

`into_wit(self)` on `EventOutcome` returns the generated WIT record with the two string lists moved in.

Providers whose `on_event` currently matches on `ProviderEvent` to fork between TimerTick and default (github, test) lose the explicit match: they move the timer-tick body into an `async fn on_event(cx, event)` returning `Result<EventOutcome>`. The async runtime already handles the Box-pin + correlation-tracking setup.

### Subtree dispatch moves into `MountRegistry::{lookup_child, list_children}`

Today the macro generates a `materialize` function that calls `MountRegistry::materialize`, which scans `#[subtree]` handlers. Post-fold, `lookup_child` and `list_children` check subtree handlers first (exact match by path), and on hit return `LookupResult::Subtree(tree_ref)` / `ListResult::Subtree(tree_ref)`. The `materialize` function in the macro output deletes, along with the `MountRegistry::materialize` method.

### `EffectFuture` renames to `CalloutFuture`

```rust
pub struct CalloutFuture<'cx, S, T> { ... }
```

All uses in `http.rs` and `git.rs` follow. The internal mechanics are unchanged.

### Provider call sites

- `cx.preload_paths(...)` → `projection.preload_many(...)` (or `projection.preload(path, bytes)` for single items).
- `cx.invalidate_path(...)` / `cx.invalidate_prefix(...)` → `outcome.invalidate_path(...)` / `outcome.invalidate_prefix(...)` on the event outcome the handler is about to return.
- `ProviderResponse::terminal(ActionResult::Ok)` in `on-event` → `Ok(EventOutcome::new())`.

Providers affected: `github` (handlers/{issues,pulls,actions}.rs, events.rs), `test` (lib.rs), `dns` (unchanged on cache, but `SingleEffect` → `Callout` rename lands in test imports).

## Host impact

### `EffectRuntime` renames and simplifies

- `EffectRuntime` → `CalloutRuntime`. All referencing sites follow (`crates/host/src/runtime/mod.rs`, `registry.rs`, `fuse/mod.rs`, tests).
- `execute_single_effect` → `execute_single_callout`. Loses the three fire-and-forget match arms (`CachePreload` / `CacheInvalidatePath` / `CacheInvalidatePrefix`) and the four dead git arms (`GitListTree` / `GitReadBlob` / `GitHeadRef` / `GitListCachedRepos`).
- `execute_batch` stays named.
- `cache_preload_files` / `cache_delete_path` / `cache_delete_prefix` stay, but are called from a new boundary-application step rather than inside `execute_single_callout`.

### `drive_effects` renames to `drive_callouts` and gains a boundary-application step

```rust
async fn drive_callouts(&self, id: u64, mut response: provider-return) -> Result<op-result> {
    loop {
        let callouts = std::mem::take(&mut response.callouts);
        let terminal = response.terminal.take();

        // Apply terminal-embedded side effects before handing back to FUSE.
        if let Some(ref t) = terminal {
            match t {
                OpResult::List(ListResult::Entries(listing)) => {
                    self.apply_preloads(&listing.preload);
                }
                OpResult::Event(event) => {
                    self.apply_invalidations(&event.invalidate_paths, &event.invalidate_prefixes);
                }
                _ => {}
            }
        }

        match (terminal, callouts.is_empty()) {
            (Some(t), _) => {
                if !callouts.is_empty() {
                    let _ = self.execute_batch(&callouts).await;
                }
                return Ok(t);
            }
            (None, true) => return Err(RuntimeError::ProviderError("empty response".into())),
            (None, false) => {
                let results = self.execute_batch(&callouts).await;
                let mut store = self.store.lock();
                response = self.bindings.omnifs_provider_resume().call_resume(
                    &mut *store, id, &results,
                )?;
            }
        }
    }
}
```

### Subtree handoff folds into browse pipeline

`try_subtree_handoff` and `call_materialize` delete. In their place, `call_lookup_child` and `call_list_children` inspect the guest's returned terminal: if it carries a subtree variant, synthesize the same `DisownedTree`-flavored downstream flow (inode with `backing_path`) that `try_subtree_handoff` used to produce. `opendir_via_provider` stops special-casing subtree paths; it just calls `call_list_children` and handles the subtree variant.

### Action-result stripping for listings

`browse_pipeline::strip_projected_files` already strips `projected_files` from `dir-entries` before caching. It gains equivalent responsibility for the subtree variant (passthrough) and for the preload field (already consumed at boundary; the dir-listing persisted in L2 should have an empty preload list).

### Dead git cleanup: exact file list

- `wit/provider.wit`: delete four arms from `callout` (after rename), delete their payload records if any (`git-tree-request`, `git-blob-request`, `git-cache-list-request` — keep `git-open-request` and `git-repo-info` which subtree handoff still uses).
- `crates/host/src/runtime/mod.rs::execute_single_callout`: delete four match arms.
- `crates/host/src/runtime/git.rs::GitExecutor`: delete `list_tree`, `read_blob`, `head_ref`, `list_cached_repos` methods and their supporting helpers (`read_blob_content`, `read_head_ref`, `list_cached_repos_generic`). Keep `open_repo` and `repo_path`, which the subtree handoff still needs to resolve `tree-ref` to a filesystem path.
- `crates/host/tests/git_executor_test.rs`: delete tests that exercise the removed methods (`test_list_tree_root`, `test_list_tree_subdir`, `test_read_blob`, `test_head_ref`, `test_unopened_repo_errors`). Keep whatever exercises `open_repo` alone, or delete the file if nothing remains.
- `crates/omnifs-sdk/src/git.rs`: delete `Builder::list_tree`, `Builder::read_blob`, `Builder::head_ref` methods and their `#[test]` cases (`open_repo_yields_git_open_effect` stays, `head_ref_returns_delivered_reference` goes). No `list_cached_repos` was exposed in the SDK builder today.

## Tests

- `crates/host/tests/provider_routes_test.rs`: `Effect` → `Callout`, `EffectResult` → `CalloutResult`, `ProviderResponse::Done` / `Effects` patterns already use struct form from the prior refactor, re-aim at `provider-return` / `op-result`. Preload-asserting tests read from `op_result.terminal` → `OpResult::List(ListResult::Entries(listing))` → `listing.preload` instead of scanning effects for `Effect::PreloadPaths`. Invalidation-asserting tests (the events-poll test) read from `OpResult::Event(outcome)` → `outcome.invalidate_prefixes` instead of scanning effects.
- `crates/host/tests/runtime_test.rs`: `test_subtree_handoff` keeps the same assertion shape but matches on `OpResult::Lookup(LookupResult::Subtree(777))` instead of `ActionResult::DisownedTree(777)`.
- `crates/omnifs-sdk/tests/error_api_test.rs`: match `ProviderReturn { terminal: Some(OpResult::Err(...)), .. }`.
- `crates/omnifs-sdk-macros/tests/path_first_provider.rs`: subtree test rewired to exercise `lookup-child` / `list-children` dispatch instead of calling `materialize` directly.

## Migration

This is a breaking WIT change and a breaking Rust SDK change. Pre-v1, no back-compat concerns. Clean cut in one PR, coordinated with a rebuild of all release wasms.

## Out of scope

1. **Per-operation WIT interface split** (`browse-lookup`, `browse-list`, etc.). Decided against: the typing gain is already captured at the Rust layer by the macro, and the dispatch cost (per-op resume, correlation-to-op tracking) is not justified by the remaining laxity. See the thread discussion if revisiting.
2. **Mid-operation partial results**. FUSE is strictly request/response per op; partial results don't reach the user. The only user-visible streaming lever is cross-call readdir pagination, tracked separately as **OFS-32**.
3. **Cross-call readdir pagination**. Tracked as **OFS-32**; depends on wiring `PageStatus::More(cursor)` end-to-end through the host, which is independent of the shape work in this doc.
4. **Per-correlation timeout for guest + git**. Separate follow-up. The effect-boundary refactor didn't address it; this doc doesn't either.
5. **`action-result` strict per-op typing at the WIT level**. Still possible to build `OpResult::Lookup(...)` from a `list-children` handler at the WIT type system layer. The macro-layer enforcement is considered sufficient.

## Behavioral notes

- **`lookup-result::not-found` drops siblings.** Today `Lookup::not_found()` builds a `lookup-result` record with `target: None` plus optionally siblings and sibling-files. Post-fold, the `not-found` arm is bare: no siblings, no sibling-files. This is intentional. The current SDK's `Lookup::not_found()` never attaches siblings; no handler uses the combination. If a future use case wants "target doesn't exist but here's context about its parent," the handler should return `entry(lookup-entry { target: <a synthetic marker> })` or a new dedicated arm — but that's a request for later, not a regression here.
- **`event-outcome` with empty invalidation lists.** Equivalent to today's `ActionResult::Ok` for `on-event`. Valid terminal; not an error.
- **Trailing effects on `err`.** `OpResult::Err(...)` carries no cache effects. If a handler errors after staging preloads/invalidations, those are dropped. Providers that want to publish and then fail must sequence: finish publishing, then return an error on a later line. This matches the effect-boundary landing where `ProviderError` collapses trailing effects on the terminal path.

## Cost

Estimated ~500-700 lines of diff total, distributed roughly:

- `wit/provider.wit`: ~100 lines (mostly renames + fold)
- `crates/host/src/runtime/{mod,browse_pipeline}.rs`: ~150 lines (callout renames, terminal-applies boundary step, materialize fold, dead-git arm removals)
- `crates/omnifs-sdk/src/{http,git,handler,cx}.rs`: ~150 lines (callout renames, `Cx` method removals, `Projection::preload*`, new `EventOutcome`, subtree dispatch in registry)
- `crates/omnifs-sdk-macros/src/provider_macro.rs`: ~50 lines (remove `materialize` generation, adjust `on-event` dispatch to wrap `EventOutcome`)
- `providers/**`: ~50 lines (call-site updates, all mechanical)
- tests: ~200 lines (match-pattern updates, subtree test rewiring)

Comparable in size to the effect-boundary refactor that just landed. No architectural surprises; every change is a direct consequence of the shape above.

## Checklist

- [ ] Edit `wit/provider.wit`: rename, fold, add fields, remove arms. Regenerate.
- [ ] Host: rename types, rewire `drive_callouts`, add `apply_preloads` / `apply_invalidations` boundary step, delete `try_subtree_handoff` / `call_materialize`, fold subtree into `call_lookup_child` / `call_list_children`, delete dead git arm handlers.
- [ ] Host `fuse`: adjust `lookup_via_provider` / `opendir_via_provider` to match new `OpResult` shape and subtree variants.
- [ ] SDK: rename `EffectFuture` → `CalloutFuture`, delete `Cx::preload_paths` / `Cx::invalidate_*`, add `Projection::preload` / `preload_many`, add `EventOutcome` type and builder, update `MountRegistry` to dispatch subtree from `lookup_child` / `list_children` and delete `MountRegistry::materialize`.
- [ ] SDK macros: stop generating `fn materialize(...)`, wrap `on-event` return in `OpResult::Event`, propagate renames.
- [ ] Providers: update `cx.preload_paths` / `cx.invalidate_*` call sites (github handlers, events.rs, test provider). Rewrap `on-event` returns.
- [ ] Tests: update match patterns for `provider-return`, `op-result`, and subtree-as-lookup-result variants.
- [ ] Re-run the docker verification loop: `just dev`, `omnifs status`, `OMNIFS_DEMO_MODE=smoke /tmp/demo.sh`, `tail /tmp/omnifs.log` for errors.
- [ ] Update `CLAUDE.md` and `AGENTS.md` terminology references (`effect` → `callout` where the distinction matters).
