# feat/migrate-kafka

Deferred: the kafka provider needs basic-auth support in the host
`AuthManager`, which is out of scope for this migration pass.

## Blocked by

- Host-side basic-auth support in `crates/host/src/auth/mod.rs` (no PR yet).
- PR #28 and PR #29 still apply as transitive blockers once basic-auth
  lands. (The `rate_limited` / `permission_denied` / `version_mismatch`
  constructors are already on `main` from the #27 refactor.)

## Status

Do not execute this plan. Revisit after basic-auth is merged into the host.
