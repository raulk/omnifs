# feat/sdk-error-constructors

## Status

IMPORTANT: the executor MUST run step 1 before doing anything else. As of the
creation of this plan (base commit `6343486`), inspection of
`crates/omnifs-sdk/src/error.rs` shows that all three constructors targeted by
this plan (`rate_limited`, `permission_denied`, `version_mismatch`) are
already present (lines 100, 124, 128) and delegate to `Self::new(...)` in the
same style as the other constructors. `ProviderErrorKind::is_retryable` at
line 35 already returns `true` for `RateLimited` and `false` for
`PermissionDenied` / `VersionMismatch`.

If step 1 reconfirms that state, follow the "no-op path" at the bottom of this
plan and close the branch without opening a PR. Only fall through to the
implementation body if a constructor is genuinely missing or a retryable
default is wrong.

## Summary

Ensure `omnifs_sdk::error::ProviderError` exposes convenience constructors for
`rate_limited`, `permission_denied`, and `version_mismatch`, mirroring the
existing `not_found` / `invalid_input` / `internal` style. The WIT `ErrorKind`
at `wit/provider.wit` and `ProviderErrorKind` at
`crates/omnifs-sdk/src/error.rs` already expose the variants. The feature is
purely about ergonomic Rust constructors on `ProviderError` so migrated
providers (linear, huggingface, crates-io) stop remapping these kinds to
`invalid_input`.

## Blocked by

None. Independent of PR #28 (`feat/sdk-http-post-support`) and PR #29
(`feat/sdk-path-rest-captures`).

## Files touched

- `crates/omnifs-sdk/src/error.rs` (add missing constructors and unit tests if
  absent; add unit tests unconditionally if they don't already exist)

## Step 1: verify current state (mandatory first step)

Before writing any code, execute:

```bash
rg -n 'pub fn (rate_limited|permission_denied|version_mismatch)\(' \
   /Users/raul/W/gvfs/crates/omnifs-sdk/src/error.rs
rg -n 'is_retryable' /Users/raul/W/gvfs/crates/omnifs-sdk/src/error.rs
```

Expected (pre-existing) output for the first command, reflecting commit
`6343486`:

```
100:    pub fn permission_denied(message: impl Into<String>) -> Self {
124:    pub fn rate_limited(message: impl Into<String>) -> Self {
128:    pub fn version_mismatch(message: impl Into<String>) -> Self {
```

And `is_retryable` at line 35 returns:

```rust
fn is_retryable(self) -> bool {
    matches!(self, Self::Network | Self::Timeout | Self::RateLimited)
}
```

Decide based on the output:

- All three constructors present AND `is_retryable` matches `RateLimited`
  only (not `PermissionDenied` or `VersionMismatch`): go to "No-op path".
- Any constructor missing or retryable default wrong: go to "Implementation".

## Implementation

Only execute if step 1 shows a gap. For each missing constructor, add it in
the `impl ProviderError` block immediately after the existing
`unimplemented` constructor (preserving the public surface ordering already
used in the file). Each constructor body must read exactly as below; do not
paraphrase, do not invent a different helper, and do not add documentation
comments that the other constructors don't carry.

```rust
pub fn rate_limited(message: impl Into<String>) -> Self {
    Self::new(ProviderErrorKind::RateLimited, message)
}

pub fn permission_denied(message: impl Into<String>) -> Self {
    Self::new(ProviderErrorKind::PermissionDenied, message)
}

pub fn version_mismatch(message: impl Into<String>) -> Self {
    Self::new(ProviderErrorKind::VersionMismatch, message)
}
```

Rationale for this exact shape: `ProviderError::new` already sets
`retryable: kind.is_retryable()`, so the constructor does not pass a
`retryable` flag explicitly. `ProviderErrorKind::is_retryable` already reads:

```rust
fn is_retryable(self) -> bool {
    matches!(self, Self::Network | Self::Timeout | Self::RateLimited)
}
```

which gives `rate_limited` a retryable default of `true` and
`permission_denied` / `version_mismatch` a retryable default of `false`, as
required.

If `is_retryable` does NOT include `Self::RateLimited` in its `matches!`
arm, add it (it is required for the retryable default below). Do not add
`PermissionDenied` or `VersionMismatch` to that arm.

If the WIT-level `ErrorKind` variants `RateLimited`, `PermissionDenied`, or
`VersionMismatch` are somehow absent from the bindings, stop and surface the
discrepancy; this plan does not cover WIT changes.

## Tests to add

Append these three unit tests at the bottom of
`crates/omnifs-sdk/src/error.rs` inside (or creating, if absent) a
`#[cfg(test)] mod tests { ... }` block. Do not duplicate an existing test
module; extend it. Assert both `.kind()` and `.is_retryable()` on each.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limited_constructor() {
        let err = ProviderError::rate_limited("github abuse limit");
        assert_eq!(err.kind(), ProviderErrorKind::RateLimited);
        assert!(err.is_retryable());
        assert_eq!(err.message(), "github abuse limit");
    }

    #[test]
    fn permission_denied_constructor() {
        let err = ProviderError::permission_denied("no read scope");
        assert_eq!(err.kind(), ProviderErrorKind::PermissionDenied);
        assert!(!err.is_retryable());
        assert_eq!(err.message(), "no read scope");
    }

    #[test]
    fn version_mismatch_constructor() {
        let err = ProviderError::version_mismatch("protocol v2 required");
        assert_eq!(err.kind(), ProviderErrorKind::VersionMismatch);
        assert!(!err.is_retryable());
        assert_eq!(err.message(), "protocol v2 required");
    }
}
```

If a `#[cfg(test)] mod tests` block already exists in `error.rs`, add the
three `#[test] fn ...` bodies inside it (without re-declaring `mod tests` or
`use super::*`) and skip any tests already present by the same name.

## Retryable defaults (per WIT semantics)

- `rate-limited` &rarr; `true` (client should back off and retry)
- `permission-denied` &rarr; `false` (user must fix credentials/scopes)
- `version-mismatch` &rarr; `false` (protocol incompatibility, not transient)

These must match what `ProviderErrorKind::is_retryable` already returns for
these three variants. Do not override `retryable` in the constructor; let
`ProviderError::new` derive it from the kind.

## Verification

Run each of these from `/Users/raul/W/gvfs` and confirm clean exit before
committing:

```bash
cargo fmt --check
cargo clippy -p omnifs-sdk -- -D warnings
cargo test -p omnifs-sdk
cargo clippy -p omnifs-provider-github --target wasm32-wasip2 -- -D warnings
cargo clippy -p omnifs-provider-dns --target wasm32-wasip2 -- -D warnings
just check-providers
```

All must pass without warnings. Do not use `--no-verify` on the commit. Do
not use `git add -A` or `git add .`; stage `crates/omnifs-sdk/src/error.rs`
explicitly.

## Commit

Single conventional commit, sentence-case body, no em dashes:

```
feat(sdk-error): add rate_limited, permission_denied, version_mismatch constructors

Migrated providers need fidelity on error kinds (linear rate-limit
handling, huggingface gated repos, crates-io deprecated/yanked flags).
Remapping these cases to invalid_input was brittle and lost retryable
semantics.
```

Stage only `crates/omnifs-sdk/src/error.rs`.

## PR

Title: `feat(sdk-error): add rate_limited, permission_denied, version_mismatch constructors`

Body (prose, no test-plan section):

```
Adds three convenience constructors on `omnifs_sdk::error::ProviderError`
mirroring the existing `not_found`/`invalid_input`/`internal` style. The
underlying `ProviderErrorKind` variants and WIT `ErrorKind` variants were
already in place; only the Rust-side constructors were missing.

## Retryable semantics

`rate_limited` defaults to retryable=true (clients should back off and
retry). `permission_denied` and `version_mismatch` default to
retryable=false (both require user or protocol action, not a retry).
`ProviderErrorKind::is_retryable` already encodes this.

## Unblocks

The linear, huggingface, and crates-io providers currently remap these
cases to `invalid_input`, which loses the retryable signal and the kind
tag surfaced to the host. With these constructors landed, those providers
can use the correct kinds directly.
```

Open against `main`. Push to `origin` (remote `raulk`). No reviewers auto-
assigned by this plan.

## No-op path

If step 1 confirms that all three constructors exist with the correct
retryable defaults AND the three unit tests above (or equivalent) are already
present, do NOT invent filler changes. Instead:

1. Leave the branch as-is (the plan commit only).
2. Tell the user the feature is already on `main` as of commit `6343486`
   (the refactor that redesigned the provider SDK), and no implementation
   PR is needed.
3. If only the unit tests are missing but the constructors exist with the
   correct retryable defaults, add only the tests using the bodies above and
   open a smaller PR with title
   `test(sdk-error): cover rate_limited, permission_denied, version_mismatch`
   and a one-paragraph body explaining the gap.

Do not open an empty or no-op PR under the `feat(sdk-error): ...` title.
