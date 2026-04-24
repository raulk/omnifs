# Provider migration orchestration

Master task list for migrating 10 WASM providers (arxiv, crates-io, gmail,
google-drive, huggingface, ipfs, linear, notion, npm, tailscale) from the
old mount-table SDK to the new path-first handler SDK on `main`.

Kafka is deferred: its REST proxy needs HTTP Basic auth, which the host's
`AuthManager` does not yet support. A stub plan on `feat/migrate-kafka`
records the deferral; no execution is scheduled.

## Ground rules

- **No direct pushes to `main`.** Every change lands via PR review.
- **No hook bypass** (`--no-verify`, `--no-gpg-sign`). If a hook fails,
  fix the cause and create a new commit.
- **No `git add .` or `git add -A`.** Stage files explicitly.
- Every sonnet executor works in its own `.worktrees/migrate-<name>`
  worktree on its own `feat/migrate-<name>` branch.

## Branch inventory

All branches below already exist on `origin` except where marked.

### Phase 1: SDK foundation

| Branch                          | PR   | Adds                                                    | Status           |
|---------------------------------|------|---------------------------------------------------------|------------------|
| `feat/sdk-http-post-support`    | #28  | `Builder::post`, `Request::body`, `Request::json`       | open for review  |
| `feat/sdk-path-rest-captures`   | #29  | `{*rest}` path captures through the handler macro       | open for review  |
| `feat/sdk-error-constructors`   | -    | Plan only. `rate_limited`/`permission_denied`/`version_mismatch` constructors already landed in #27. PLAN.md documents the verify-first no-op path; the branch is archival unless tests are missing. | plan pushed, no PR expected |

### Phase 2: Provider migration plans

Each branch carries a single `PLAN.md` describing a sonnet-executable
migration of one provider. Implementation happens on the same branch
when the executor runs.

| Branch                          | Batch | Provider     | Scope notes                                             |
|---------------------------------|-------|--------------|---------------------------------------------------------|
| `feat/migrate-tailscale`        | F1    | tailscale    | smallest; single handler module                         |
| `feat/migrate-npm`              | F1    | npm          | scoped/unscoped split routes                            |
| `feat/migrate-crates-io`        | F1    | crates-io    | approved hard-salvage of provider files from old branch |
| `feat/migrate-notion`           | F2    | notion       | POST GraphQL-adjacent + auth                            |
| `feat/migrate-gmail`            | F2    | gmail        | large messages, triplicate handlers for `/messages/{id}`|
| `feat/migrate-google-drive`     | F2    | google-drive | export MIME routing, 64 KiB eager cap                   |
| `feat/migrate-arxiv`            | F3    | arxiv        | ~50 handlers across 5 modules                           |
| `feat/migrate-huggingface`      | F3    | huggingface  | timer-tick polling, git subtree handoffs                |
| `feat/migrate-linear`           | F3    | linear       | GraphQL POSTs, etag polling, drafts deferred            |
| `feat/migrate-ipfs`             | F4    | ipfs         | deep traversal via `{*path}` (depends on #29)           |
| `feat/migrate-kafka`            | -     | kafka        | deferred stub; no execution                             |

### Phase 0: orchestration (this doc)

| Branch                          | Carries                                                                 |
|---------------------------------|-------------------------------------------------------------------------|
| `docs/migration-orchestration`  | this ORCHESTRATION.md, pushed to origin as the coordination reference   |

## Dependency graph

```
PR #28 ──┐
         ├── merged to main ──→ F1, F2, F3, F4 may begin
PR #29 ──┘

Track D: already on main (no PR). Not a blocker.
```

All Phase 2 work depends on #28 and #29 landing on `main`. Within Phase
2, batches are concurrency caps (3-4 sonnet agents at a time), not
hard ordering. F4 additionally requires #29's `{*rest}` support.

No provider migration depends on any other provider migration.

## Landing sequence

### Phase 1 steps

| Step | Action                                                    | Gate                                |
|------|-----------------------------------------------------------|-------------------------------------|
| 1.1  | Human review + merge PR #28                               | reviewer approval + green CI        |
| 1.2  | Human review + merge PR #29                               | reviewer approval + green CI        |
| 1.3  | (optional) verify Track D via PLAN.md step 1;             | if constructors missing → open PR;  |
|      | expected outcome: constructors already present, no-op     | else close branch                   |

1.1 and 1.2 can land in either order.

### Phase 2 steps

Phase 2 starts only after both #28 and #29 are merged into `main`.

**Batch F1** (dispatch 3 sonnet executors in parallel):

| Step  | Branch                     | Provider     |
|-------|----------------------------|--------------|
| 2.F1a | `feat/migrate-tailscale`   | tailscale    |
| 2.F1b | `feat/migrate-npm`         | npm          |
| 2.F1c | `feat/migrate-crates-io`   | crates-io    |

Each executor rebases its branch onto the latest `main` (which now
contains #28 and #29), follows its `PLAN.md` end-to-end, commits
implementation on the same branch, runs the verification suite, and
opens a PR.

**Batch F2** (3 parallel executors, start when F1 is dispatched or
merged -- no hard gate):

| Step  | Branch                     | Provider      |
|-------|----------------------------|---------------|
| 2.F2a | `feat/migrate-notion`      | notion        |
| 2.F2b | `feat/migrate-gmail`       | gmail         |
| 2.F2c | `feat/migrate-google-drive`| google-drive  |

**Batch F3** (3 parallel executors):

| Step  | Branch                        | Provider     |
|-------|-------------------------------|--------------|
| 2.F3a | `feat/migrate-arxiv`          | arxiv        |
| 2.F3b | `feat/migrate-huggingface`    | huggingface  |
| 2.F3c | `feat/migrate-linear`         | linear       |

**Batch F4** (1 executor):

| Step  | Branch                | Provider |
|-------|-----------------------|----------|
| 2.F4  | `feat/migrate-ipfs`   | ipfs     |

### Phase 3: merges and cleanup

Provider PRs land on `main` independently after human review. They do
not conflict with each other (disjoint `providers/<name>/` subtrees,
plus a shared but additive edit to workspace `Cargo.toml` `members`).

After each `feat/migrate-<name>` PR merges:

- Delete the corresponding `wip/provider-<name>-impl` branch.
- Remove `.worktrees/providers/<name>` and `.worktrees/migrate-<name>`.

Once all provider PRs land:

- Delete `feat/sdk-error-constructors` (archival; no PR).
- Either convert the `feat/migrate-kafka` stub into a tracking issue
  and delete the branch, or leave it open as a future-work marker.
- Delete `docs/migration-orchestration` once the migration is fully
  landed; retain the commit history on main if the doc is desired
  long-term (move into `docs/` on main via a separate PR first).

## Per-executor contract

Each sonnet migration executor receives:

1. The branch name (`feat/migrate-<provider>`).
2. The path to the old provider worktree where it may pull source from:
   `/Users/raul/W/gvfs/.worktrees/providers/<provider>/providers/<provider>/`.
3. The old wip branch name: `wip/provider-<provider>-impl`.

Its workflow:

```bash
# 1. Rebase the feat branch onto latest main
git -C /Users/raul/W/gvfs fetch origin
git -C /Users/raul/W/gvfs worktree add .worktrees/migrate-<name> feat/migrate-<name>
cd .worktrees/migrate-<name>
git rebase origin/main

# 2. Read PLAN.md, execute step by step
cat PLAN.md

# 3. Port provider source per the plan's instructions
git checkout wip/provider-<name>-impl -- providers/<name>/

# 4. Apply rewrites, add workspace member, etc., per PLAN.md

# 5. Verify
cargo fmt --check
cargo clippy -p omnifs-provider-<name> --target wasm32-wasip2 -- -D warnings
cargo test -p omnifs-provider-<name> --target wasm32-wasip2 --no-run
just check-providers

# 6. Commit (conventional), push, open PR
git push -u origin HEAD
gh pr create --title "feat(<name>): migrate provider to path-first handler SDK" \
             --body "..."
```

## Known corrections to the plans

The corrected plans on each `feat/migrate-*` branch apply these fixes
to the original drafts in `.worktrees/providers/*/MIGRATION_PLAN.md`:

1. **Auth via capabilities + host AuthManager.** Providers declare
   `auth_types` + `domains` in `capabilities()`; the host injects
   `Authorization` headers per-domain. Tokens are NOT threaded through
   provider state. Canonical reference: `providers/github/src/http_ext.rs`.
2. **POST shape uses #28 builder surface.** Any `Callout::Fetch`
   workaround is replaced with
   `cx.http().post(url).header(...).json(&body)?.send_body().await`.
3. **ipfs deep traversal restored.** Uses `{*path}` from #29.
4. **Error kinds use real constructors.** Linear/huggingface/crates-io
   use `ProviderError::rate_limited`, `::permission_denied`,
   `::version_mismatch` directly. (Already on main; Track D is not a
   dependency.)
5. **crates-io hard-salvage.** The approved path is
   `git checkout wip/provider-crates-io-impl -- providers/crates-io/`,
   not a full branch merge.

## Prompt for a new Claude session to orchestrate

Paste the following into a fresh Claude Code session in
`/Users/raul/W/gvfs` on `main`:

> You are orchestrating the provider migration described in
> `docs/migration-orchestration` (branch). Read `ORCHESTRATION.md` on
> that branch and execute the landing sequence.
>
> Pre-flight:
> 1. `git fetch origin` and confirm PRs #28 and #29 are both merged
>    into `main`. If either is not merged, STOP and surface to the
>    user; do not dispatch any Phase 2 work until both are in.
> 2. Verify `crates/omnifs-sdk/src/error.rs` contains
>    `rate_limited`, `permission_denied`, `version_mismatch` (should
>    already be present from #27). If not, dispatch the
>    `feat/sdk-error-constructors` executor before Phase 2.
>
> Phase 2 dispatch rules:
> - Dispatch sonnet executors in batches F1 → F2 → F3 → F4.
> - Within a batch, run agents in parallel (3-4 concurrent max).
> - Move from F1 to F2 once all F1 agents have OPENED PRs (not
>   necessarily merged). Same for F2→F3, F3→F4.
> - Each executor prompt: "You are migrating the `<name>` provider.
>   Create a worktree at `.worktrees/migrate-<name>` from branch
>   `feat/migrate-<name>`, rebase onto `origin/main`, and execute
>   `PLAN.md` end-to-end. Port provider source from branch
>   `wip/provider-<name>-impl` using `git checkout
>   wip/provider-<name>-impl -- providers/<name>/`. Run the full
>   verification suite before pushing. Open PR titled
>   `feat(<name>): migrate provider to path-first handler SDK` with
>   a body that links to the branch PLAN.md and reports verification
>   results. Do not merge the PR."
> - Use the `general-purpose` agent type with `model: "sonnet"` and
>   `run_in_background: true`. When an agent completes, report the
>   PR URL, verification state, and any deviations. Do not re-dispatch
>   failed agents without explicit user approval.
>
> Post-flight:
> - Report a final summary: PRs opened, verification pass/fail per
>   provider, any deviations from plan.
> - Do not merge PRs yourself; leave for human review.
> - Do not push to `main` directly under any circumstance.
> - If `just` is missing, note in PR bodies and skip that one check;
>   all other verification must pass.
