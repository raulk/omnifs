# Implementation prompt

You are implementing a protocol refactor in the omnifs codebase. Working directory: `/Users/raul/W/gvfs`. The work is fully specified in `design/protocol-shape.md` — read that first, read `CLAUDE.md` and `AGENTS.md` for project conventions, and follow both exactly.

## Task

Implement `design/protocol-shape.md` end-to-end. The doc specifies: rename the WIT types (`effect`/`effect-result`/`action-result`/`provider-response` → `callout`/`callout-result`/`op-result`/`provider-return`), fold fire-and-forget effects into terminal payloads (preload into `dir-listing`, invalidations into a new `event-outcome` record), fold `materialize` into `lookup-child` and `list-children` return variants, delete the four dead git callouts, collapse variant sprawl in `op-result`, rename the corresponding Rust types (`EffectRuntime` → `CalloutRuntime`, `EffectFuture` → `CalloutFuture`, `drive_effects` → `drive_callouts`, etc.), and update all providers and tests accordingly.

The "Target WIT" section gives the concrete target shapes. The "Rename table" is exhaustive. The "SDK impact", "Host impact", "Dead git cleanup", and "Tests" sections name the files to edit and what changes. The "Checklist" at the end is your implementation order. The "Out of scope" and "Behavioral notes" sections are hard fences — do not expand past them.

## Context you need

- Pre-v1 codebase, breaking changes are fine, no back-compat required.
- WIT lives at `wit/provider.wit`. The SDK regenerates bindings via `wit_bindgen::generate!` in `crates/omnifs-sdk/src/lib.rs`; the host regenerates via `wasmtime::component::bindgen!` in `crates/host/src/lib.rs`. Both pick up WIT changes automatically on the next build.
- Providers build for `wasm32-wasip2`. See `CLAUDE.md` for the exact cargo commands.
- Tests that construct provider WIT types by name are in `crates/host/tests/provider_routes_test.rs` (largest), `crates/host/tests/runtime_test.rs`, `crates/omnifs-sdk/tests/error_api_test.rs`, and `crates/omnifs-sdk-macros/tests/path_first_provider.rs`.
- The most recent effect-boundary refactor (commits on the current branch `feat/provider-sdk-dx-redesign`) is the template: same kind of cross-cutting rename + shape change across WIT, host, SDK, macros, providers, and tests. Read the last ~15 commits to see the pattern, especially the test pattern updates using `ProviderResponse { terminal: Some(...), .. }` struct patterns.

## Verification loop (non-negotiable)

After implementation, all of the following must pass with no warnings:

```bash
cargo fmt --all
cargo clippy --workspace -- -D warnings
cargo clippy -p omnifs-provider-github -p omnifs-provider-dns -p test-provider --target wasm32-wasip2 -- -D warnings
cargo test --workspace
cargo build --release --target wasm32-wasip2 -p omnifs-provider-github -p omnifs-provider-dns -p test-provider
```

Then the end-to-end docker verification (from `AGENTS.md`):

```bash
just dev
docker exec omnifs /bin/zsh -lc 'omnifs status'
docker exec omnifs /bin/zsh -lc 'OMNIFS_DEMO_MODE=smoke /tmp/demo.sh'
docker exec omnifs /bin/zsh -lc 'tail -n 200 /tmp/omnifs.log'
docker compose down
```

The `omnifs status` should show both providers ready. The demo script should list issues, dump an issue body, list action runs, show a run status — all without errors in `/tmp/omnifs.log` beyond the documented-benign FUSE `access(...)` / `flush(...)` warnings.

## Ground rules

1. **Follow the doc, not your own instincts.** If the doc says `OpResult::Event(outcome)`, don't invent `OpResult::EventDone(...)`. If it says `CalloutRuntime`, don't keep `EffectRuntime`. Every rename and every variant name is deliberate; changes should be raised as questions, not silently adjusted.

2. **Match the landed refactor's patterns.** Use struct patterns for `provider-return` matches (`ProviderReturn { terminal: Some(OpResult::Lookup(...)), .. }`). Use the host-side `expect_terminal()` / `expect_effects()` / `is_suspended()` helpers in tests where they clarify (already on the host's `ProviderResponse` type; rename if touched). Don't introduce new test helpers unless a pattern appears 5+ times.

3. **Preserve the preload/invalidate ordering invariant.** The effect-boundary refactor guarantees that cache effects land before the host returns the terminal to FUSE or dispatches sibling callouts. The fold moves where the invariant is enforced (from `execute_single_effect` to the new boundary-apply step in `drive_callouts`), but the observable behavior must be identical. Concretely: the next FUSE operation after a `list-children` terminal sees the listing's preloads already in the cache; the next operation after an `on-event` terminal sees invalidations applied.

4. **`cx.preload_paths`, `cx.invalidate_path`, `cx.invalidate_prefix` are gone.** Any provider code that still calls them is a compile error. Fix the call sites per the doc's SDK impact section, not by re-adding the methods.

5. **`materialize` method is gone.** Any host code calling `call_materialize` is a compile error. Fold into the browse pipeline per the doc. The `#[subtree]` handler macro attribute stays; only the WIT method dispatch moves.

6. **Subagent and search discipline.** For understanding the current code before editing, dispatch Explore subagents with specific questions rather than scanning the tree yourself. Big grep targets include `SingleEffect`, `CachePreload`, `materialize`, `DisownedTree`, `ProviderResponse::Done` (the last should no longer exist after the prior refactor; sanity-check).

7. **Commit discipline.** One coherent commit per logical unit (WIT change, host side, SDK core, SDK macro, github provider, dns provider, test provider, tests, docs). Conventional-commits format per the user's `~/.claude/rules/conventional-commits.md`. Titles under 70 chars.

8. **Don't touch** the four "Out of scope" items in the doc. If something looks tempting, add it to a follow-up list and raise at the end.

## Deliverables

1. Commits implementing the refactor on the current branch (or a new branch off `feat/provider-sdk-dx-redesign` if that's already merged when you start; check).
2. All checklist items in the design doc ticked.
3. Clean `cargo clippy` + `cargo test --workspace` + docker smoke demo.
4. A short summary at the end: commits landed, total line count (`git diff --stat` against the base), any deviations from the doc with justification, anything flagged for follow-up.

## Ask, don't guess

If the design doc is ambiguous on any point, or the code's current state conflicts with an assumption in the doc, ask before proceeding. Specifically: if you find an existing usage pattern not mentioned in the SDK impact section, or if a test's current structure doesn't obviously map to the new shape, raise it.

Start by reading `design/protocol-shape.md` top-to-bottom, then `CLAUDE.md` and `AGENTS.md`, then the last ~15 git commits on the current branch. Then work the checklist.
