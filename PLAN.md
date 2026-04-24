# feat/migrate-kafka

Deferred: the kafka provider needs basic-auth support in the host
`AuthManager`, which is out of scope for this migration pass.

## Blocked by

- Host-side basic-auth support in `crates/host/src/auth/mod.rs` (no PR yet).
- PR #28, PR #29, PR TBD `feat/sdk-error-constructors` still apply as
  transitive blockers once basic-auth lands.

## Status

Do not execute this plan. Revisit after basic-auth is merged into the host.
