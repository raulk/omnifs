# Tiered browse cache implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the WASM browse regression by implementing a two-tier host-owned browse cache with projected-file pre-materialization, so `rg */title */body` after one directory listing hits local cache instead of making hundreds of upstream GitHub fetches, while preserving freshness and cache-only fallback behavior.

**Architecture:** L0 is an in-memory, inode-keyed, byte-weighted moka cache per mount session in FuseFs. L2 is a durable, path-keyed redb database per provider instance in EffectRuntime. List operations pre-materialize projected files (title, body, state, user) into L2 so subsequent reads bypass the provider entirely. Provider-driven prefix invalidation keeps host tiers fresh when the GitHub event poller sees upstream changes. Existing per-handle caches (`file_cache`, `dir_snapshots`) remain as the innermost layer. The provider-side JSON cache stays in place for cache-only and stale-on-error fallback in this phase; generic provider write-back for comments, diffs, and logs is follow-up work.

**Tech Stack:** moka (L0 in-memory cache), redb (L2 durable cache), postcard+serde (payload serialization), existing wasmtime/fuser/dashmap stack.

**Design document:** `docs/internal/2026-04-13-tiered-browse-cache-design-FINAL.md`

---

## File structure

### New files

| File | Responsibility |
|------|----------------|
| `crates/host/src/cache/mod.rs` | RecordKind, CacheRecord, payload types, TTL constants, serialization |
| `crates/host/src/cache/l0.rs` | BrowseCacheL0: moka wrapper with L0Key, weighted eviction, custom expiry |
| `crates/host/src/cache/l2.rs` | BrowseCacheL2: redb wrapper with table definitions, get/put/put_batch |
| `crates/host/tests/cache_record_test.rs` | CacheRecord and payload serialization round-trip tests |
| `crates/host/tests/cache_l0_test.rs` | L0 cache operations: insert, eviction, TTL, skip threshold |
| `crates/host/tests/cache_l2_test.rs` | L2 cache operations: put/get, batch, expiry, table routing |

### Modified files

| File | Changes |
|------|---------|
| `wit/provider.wit` | Add `projected-file` record and `projected-files` field on `dir-entry`; add cache invalidation effect; bump package version |
| `Cargo.toml` (workspace) | Add moka and redb to workspace dependencies |
| `crates/host/Cargo.toml` | Add moka and redb dependencies |
| `crates/host/src/lib.rs` | Add `pub mod cache;` |
| `crates/host/src/runtime/mod.rs` | Add BrowseCacheL2 field, cache_get/put/put_batch/delete_prefix methods, projected-files extraction in call_list_entries |
| `crates/host/src/fuse/mod.rs` | Add per-mount L0 HashMap, L0 helper methods, L0/L2 checks in lookup/opendir/read, invalidate L0 entries on runtime-issued prefix invalidations |
| `crates/host/src/registry.rs` | Thread cache_dir parameter to EffectRuntime::new |
| `crates/cli/src/main.rs` | Pass cache_dir to registry |
| `providers/github/src/browse/mod.rs` | Add projected_files: None to dir_entry/file_entry helpers |
| `providers/github/src/browse/routing.rs` | Add projected_files: None to all DirEntry constructors |
| `providers/github/src/browse/resources.rs` | Populate projected_files in finalize_search_results; add projected_files: None to remaining constructors |
| `providers/github/src/browse/files.rs` | Add projected_files: None to comment DirEntry constructors |
| `providers/github/src/browse/events.rs` | Emit host cache invalidation for repo/resource prefixes when events indicate stale browse data |
| `providers/test/src/lib.rs` | Add projected_files: None to all DirEntry constructors so provider workspace still builds |

---

### Task 1: WIT change and bindings

**Files:**
- Modify: `wit/provider.wit:1` (package version), `wit/provider.wit:14-31` (single-effect/single-effect-result), `wit/provider.wit:194-198` (dir-entry)
- Modify: `providers/github/src/browse/mod.rs:193-207`
- Modify: `providers/github/src/browse/routing.rs` (all DirEntry constructors)
- Modify: `providers/github/src/browse/resources.rs` (all DirEntry constructors)
- Modify: `providers/github/src/browse/files.rs` (DirEntry constructors)
- Modify: `providers/test/src/lib.rs` (all DirEntry constructors)

- [ ] **Step 1: Update WIT definitions**

In `wit/provider.wit`, bump the package version and add the new browse-cache types:

```wit
package omnifs:provider@0.2.0;
```

After the existing `dir-entry` record (line 194), add the `projected-file` record and update `dir-entry`:

```wit
record dir-entry {
    name: string,
    kind: entry-kind,
    size: option<u64>,
    projected-files: option<list<projected-file>>,
}

record projected-file {
    name: string,
    content: list<u8>,
}
```

Also add a host cache invalidation effect and ack result:

```wit
variant single-effect {
    // ... existing variants ...
    cache-invalidate-prefix(cache-invalidate-request),
}

variant single-effect-result {
    // ... existing variants ...
    cache-ok,
}

record cache-invalidate-request {
    prefix: string,
}
```

- [ ] **Step 2: Add DirEntry helper to provider**

In `providers/github/src/browse/mod.rs`, replace the `dir_entry` and `file_entry` helpers with versions that include the new field:

```rust
pub(crate) fn dir_entry(name: &str) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::DirEntryOption(Some(DirEntry {
        name: name.to_string(),
        kind: EntryKind::Directory,
        size: None,
        projected_files: None,
    })))
}

pub(crate) fn file_entry(name: &str) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::DirEntryOption(Some(DirEntry {
        name: name.to_string(),
        kind: EntryKind::File,
        size: Some(4096),
        projected_files: None,
    })))
}
```

- [ ] **Step 3: Fix all DirEntry struct literals in routing.rs**

Add `projected_files: None` to every `DirEntry { ... }` literal in `providers/github/src/browse/routing.rs`. There are constructors at approximately lines 204, 236-255, 264-275, 278, 329-363, 401-406, 428-443. Each one needs the field added. Example for the repo-level listing (line 235):

```rust
ProviderResponse::Done(ActionResult::DirEntries(vec![
    DirEntry {
        name: "_repo".to_string(),
        kind: EntryKind::Directory,
        size: None,
        projected_files: None,
    },
    // ... same for _issues, _prs, _actions
]))
```

- [ ] **Step 4: Fix all DirEntry struct literals in resources.rs**

Add `projected_files: None` to every `DirEntry { ... }` in `providers/github/src/browse/resources.rs`. Key sites: `resume_cached_repos` (~lines 37, 53), `resume_repo_pages` (~line 237), `resume_list_first_page` (~line 334), `finalize_search_results` (~line 458), `finalize_cached_resource_list` (~line 503), `finalize_cached_runs_list` (~line 525).

- [ ] **Step 5: Fix DirEntry struct literals in files.rs**

Add `projected_files: None` to every `DirEntry { ... }` in `providers/github/src/browse/files.rs`. Key site: `list_cached_comments` and `resume_comments` (~lines 262, 316).

- [ ] **Step 6: Fix DirEntry struct literals in providers/test**

Add `projected_files: None` to every `DirEntry { ... }` in `providers/test/src/lib.rs`, including both `resolve_entry` and `list_entries`.

- [ ] **Step 7: Verify builds**

Run: `just build-providers`
Expected: Provider components build cleanly, including `github` and `test`.

Run: `cargo build --workspace`
Expected: Host workspace still builds against the updated generated bindings.

- [ ] **Step 8: Commit**

```bash
git add wit/provider.wit providers/github/src/browse/ providers/test/src/lib.rs
git commit -m "feat(wit): add projected files and cache invalidation effect

Bump WIT package to 0.2.0. DirEntry now carries an optional list of
projected files so list responses can pre-materialize follow-up read
data, and providers can explicitly invalidate host cache prefixes.
All provider DirEntry construction sites updated with projected_files: None."
```

---

### Task 2: Cache record types

**Files:**
- Modify: `Cargo.toml` (workspace deps: serde, postcard)
- Modify: `crates/host/Cargo.toml` (add serde, postcard)
- Create: `crates/host/src/cache/mod.rs`
- Modify: `crates/host/src/lib.rs`
- Create: `crates/host/tests/cache_record_test.rs`

- [ ] **Step 1: Add serde and postcard dependencies**

In workspace `Cargo.toml`, add under `[workspace.dependencies]`:

```toml
serde = { version = "1", features = ["derive"] }
postcard = { version = "1", features = ["alloc"] }
```

In `crates/host/Cargo.toml`, add under `[dependencies]`:

```toml
serde = { workspace = true }
postcard = { workspace = true }
```

- [ ] **Step 2: Write the failing test for CacheRecord serialization**

Create `crates/host/tests/cache_record_test.rs`:

```rust
use omnifs_host::cache::{
    AttrPayload, CacheRecord, DirentsPayload, DirentRecord, EntryKindCache,
    LookupPayload, RecordKind,
};

#[test]
fn cache_record_round_trip() {
    let record = CacheRecord {
        schema_version: 1,
        kind: RecordKind::Attr,
        created_at: 1000,
        expires_at: 2000,
        payload: vec![1, 2, 3, 4],
    };
    let bytes = record.serialize();
    let decoded = CacheRecord::deserialize(&bytes).unwrap();
    assert_eq!(decoded.schema_version, 1);
    assert_eq!(decoded.kind, RecordKind::Attr);
    assert_eq!(decoded.created_at, 1000);
    assert_eq!(decoded.expires_at, 2000);
    assert_eq!(decoded.payload, vec![1, 2, 3, 4]);
}

#[test]
fn cache_record_rejects_unknown_schema_version() {
    let mut bytes = CacheRecord {
        schema_version: 1,
        kind: RecordKind::File,
        created_at: 0,
        expires_at: 0,
        payload: vec![],
    }
    .serialize();
    bytes[0] = 99; // corrupt schema version
    assert!(CacheRecord::deserialize(&bytes).is_none());
}

#[test]
fn lookup_payload_positive_round_trip() {
    let payload = LookupPayload::Positive {
        kind: EntryKindCache::File,
        size: 42,
    };
    let bytes = payload.serialize();
    let decoded = LookupPayload::deserialize(&bytes).unwrap();
    assert!(matches!(
        decoded,
        LookupPayload::Positive { kind: EntryKindCache::File, size: 42 }
    ));
}

#[test]
fn lookup_payload_negative_round_trip() {
    let bytes = LookupPayload::Negative.serialize();
    let decoded = LookupPayload::deserialize(&bytes).unwrap();
    assert!(matches!(decoded, LookupPayload::Negative));
}

#[test]
fn attr_payload_round_trip() {
    let payload = AttrPayload {
        kind: EntryKindCache::Directory,
        size: 0,
    };
    let bytes = payload.serialize();
    let decoded = AttrPayload::deserialize(&bytes).unwrap();
    assert_eq!(decoded.kind, EntryKindCache::Directory);
    assert_eq!(decoded.size, 0);
}

#[test]
fn dirents_payload_round_trip() {
    let payload = DirentsPayload {
        entries: vec![
            DirentRecord {
                name: "title".to_string(),
                kind: EntryKindCache::File,
                size: 128,
            },
            DirentRecord {
                name: "comments".to_string(),
                kind: EntryKindCache::Directory,
                size: 0,
            },
        ],
    };
    let bytes = payload.serialize();
    let decoded = DirentsPayload::deserialize(&bytes).unwrap();
    assert_eq!(decoded.entries.len(), 2);
    assert_eq!(decoded.entries[0].name, "title");
    assert_eq!(decoded.entries[0].size, 128);
    assert_eq!(decoded.entries[1].name, "comments");
    assert_eq!(decoded.entries[1].kind, EntryKindCache::Directory);
}

#[test]
fn cache_record_is_expired() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let expired = CacheRecord {
        schema_version: 1,
        kind: RecordKind::Attr,
        created_at: now - 1000,
        expires_at: now - 1,
        payload: vec![],
    };
    assert!(expired.is_expired());

    let fresh = CacheRecord {
        schema_version: 1,
        kind: RecordKind::Attr,
        created_at: now,
        expires_at: now + 300,
        payload: vec![],
    };
    assert!(!fresh.is_expired());
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p omnifs-host --test cache_record_test`
Expected: Compilation error, module `cache` not found.

- [ ] **Step 4: Create cache module with record types**

Add `pub mod cache;` to `crates/host/src/lib.rs` (after line 19, unconditionally compiled).

Create `crates/host/src/cache/mod.rs`:

```rust
//! Host browse cache types and serialization.
//!
//! Defines the shared types used by both L0 (in-memory moka) and
//! L2 (durable redb) cache tiers.
//!
//! Submodule declarations (`pub mod l0`, `pub mod l2`) are added
//! incrementally in Tasks 3 and 4 as each tier is implemented.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const SCHEMA_VERSION: u8 = 1;

/// TTL constants by record class.
pub mod ttl {
    use std::time::Duration;
    pub const DIRENTS: Duration = Duration::from_secs(120);
    pub const ATTR: Duration = Duration::from_secs(300);
    pub const LOOKUP_POSITIVE: Duration = Duration::from_secs(300);
    pub const LOOKUP_NEGATIVE: Duration = Duration::from_secs(30);
    pub const PROJECTED_FILE: Duration = Duration::from_secs(600);
    pub const BULK_FILE: Duration = Duration::from_secs(3600);
}

/// L0 sizing constants.
pub const L0_MAX_WEIGHT: u64 = 32 * 1024 * 1024; // 32 MiB per provider instance
pub const L0_SKIP_THRESHOLD: usize = 256 * 1024;  // 256 KiB

/// L2 table routing threshold.
pub const L2_BULK_THRESHOLD: usize = 64 * 1024; // 64 KiB

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RecordKind {
    Lookup = 0,
    Attr = 1,
    Dirents = 2,
    File = 3,
}

impl RecordKind {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Lookup),
            1 => Some(Self::Attr),
            2 => Some(Self::Dirents),
            3 => Some(Self::File),
            _ => None,
        }
    }
}

/// Mirror of WIT EntryKind for cache payloads, avoiding a dependency
/// on the generated WIT types in the cache module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum EntryKindCache {
    Directory = 0,
    File = 1,
}

// No manual from_u8 needed; serde handles deserialization via postcard.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheRecord {
    pub schema_version: u8,
    pub kind: RecordKind,
    pub created_at: u64,
    pub expires_at: u64,
    pub payload: Vec<u8>,
}

impl CacheRecord {
    /// Create a new record with the given kind, TTL, and payload.
    pub fn new(kind: RecordKind, ttl: Duration, payload: Vec<u8>) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            schema_version: SCHEMA_VERSION,
            kind,
            created_at: now,
            expires_at: now + ttl.as_secs(),
            payload,
        }
    }

    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now >= self.expires_at
    }

    pub fn ttl_duration(&self) -> Duration {
        Duration::from_secs(self.expires_at.saturating_sub(self.created_at))
    }

    /// Serialize to bytes: [schema_version:1][kind:1][created_at:8][expires_at:8][payload:*]
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(18 + self.payload.len());
        buf.push(self.schema_version);
        buf.push(self.kind as u8);
        buf.extend_from_slice(&self.created_at.to_be_bytes());
        buf.extend_from_slice(&self.expires_at.to_be_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Deserialize from bytes. Returns None if schema version is unrecognized.
    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 18 {
            return None;
        }
        let schema_version = bytes[0];
        if schema_version != SCHEMA_VERSION {
            return None;
        }
        let kind = RecordKind::from_u8(bytes[1])?;
        let created_at = u64::from_be_bytes(bytes[2..10].try_into().ok()?);
        let expires_at = u64::from_be_bytes(bytes[10..18].try_into().ok()?);
        let payload = bytes[18..].to_vec();
        Some(Self {
            schema_version,
            kind,
            created_at,
            expires_at,
            payload,
        })
    }
}

// --- Payload types (serialized via postcard) ---

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LookupPayload {
    Positive { kind: EntryKindCache, size: u64 },
    Negative,
}

impl LookupPayload {
    pub fn serialize(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("LookupPayload serialization is infallible")
    }

    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AttrPayload {
    pub kind: EntryKindCache,
    pub size: u64,
}

impl AttrPayload {
    pub fn serialize(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("AttrPayload serialization is infallible")
    }

    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DirentRecord {
    pub name: String,
    pub kind: EntryKindCache,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DirentsPayload {
    pub entries: Vec<DirentRecord>,
}

impl DirentsPayload {
    pub fn serialize(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("DirentsPayload serialization is infallible")
    }

    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes).ok()
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p omnifs-host --test cache_record_test`
Expected: All 7 tests pass.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/host/Cargo.toml crates/host/src/cache/mod.rs crates/host/src/lib.rs crates/host/tests/cache_record_test.rs
git commit -m "feat(cache): add cache record types and serialization

RecordKind, CacheRecord with manual 18-byte header. Payload types
(LookupPayload, AttrPayload, DirentsPayload) use postcard for
compact serde-based serialization. TTL constants per record class.
EntryKindCache mirrors WIT EntryKind for cache-module independence."
```

---

### Task 3: L2 browse cache (redb)

**Files:**
- Modify: `Cargo.toml` (workspace deps)
- Modify: `crates/host/Cargo.toml`
- Modify: `crates/host/src/cache/mod.rs` (add `pub mod l2;`)
- Create: `crates/host/src/cache/l2.rs`
- Create: `crates/host/tests/cache_l2_test.rs`

- [ ] **Step 1: Add redb dependency**

In workspace `Cargo.toml`, add under `[workspace.dependencies]`:

```toml
redb = "2"
```

In `crates/host/Cargo.toml`, add under `[dependencies]`:

```toml
redb = { workspace = true }
```

- [ ] **Step 2: Register L2 submodule in cache/mod.rs**

Add `pub mod l2;` to `crates/host/src/cache/mod.rs` (after the `L2_BULK_THRESHOLD` constant):

```rust
pub mod l2;
```

- [ ] **Step 3: Write the failing test for L2 operations**

Create `crates/host/tests/cache_l2_test.rs`:

```rust
use omnifs_host::cache::l2::BrowseCacheL2;
use omnifs_host::cache::{CacheRecord, RecordKind, ttl};

#[test]
fn l2_put_get_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("browse.redb");
    let l2 = BrowseCacheL2::open(&db_path).unwrap();

    let record = CacheRecord::new(RecordKind::Attr, ttl::ATTR, vec![1, 0, 0, 0, 0, 0, 0, 0, 42]);
    l2.put("owner/repo/_issues/_open/1/title", RecordKind::Attr, &record).unwrap();

    let got = l2.get("owner/repo/_issues/_open/1/title", RecordKind::Attr).unwrap();
    assert!(got.is_some());
    let got = got.unwrap();
    assert_eq!(got.kind, RecordKind::Attr);
    assert_eq!(got.payload, vec![1, 0, 0, 0, 0, 0, 0, 0, 42]);
}

#[test]
fn l2_get_miss() {
    let dir = tempfile::tempdir().unwrap();
    let l2 = BrowseCacheL2::open(&dir.path().join("browse.redb")).unwrap();
    assert!(l2.get("nonexistent", RecordKind::Lookup).unwrap().is_none());
}

#[test]
fn l2_expired_record_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let l2 = BrowseCacheL2::open(&dir.path().join("browse.redb")).unwrap();

    // Create an already-expired record.
    let record = CacheRecord {
        schema_version: 1,
        kind: RecordKind::Lookup,
        created_at: 0,
        expires_at: 1, // expired in 1970
        payload: vec![0],
    };
    l2.put("expired/path", RecordKind::Lookup, &record).unwrap();
    assert!(l2.get("expired/path", RecordKind::Lookup).unwrap().is_none());
}

#[test]
fn l2_file_small_goes_to_content_table() {
    let dir = tempfile::tempdir().unwrap();
    let l2 = BrowseCacheL2::open(&dir.path().join("browse.redb")).unwrap();

    let small = vec![0u8; 1024]; // 1 KiB, below 64 KiB threshold
    let record = CacheRecord::new(RecordKind::File, ttl::PROJECTED_FILE, small.clone());
    l2.put("path/to/title", RecordKind::File, &record).unwrap();

    let got = l2.get("path/to/title", RecordKind::File).unwrap().unwrap();
    assert_eq!(got.payload, small);
}

#[test]
fn l2_file_large_goes_to_bulk_table() {
    let dir = tempfile::tempdir().unwrap();
    let l2 = BrowseCacheL2::open(&dir.path().join("browse.redb")).unwrap();

    let large = vec![0u8; 100_000]; // 100 KiB, above 64 KiB threshold
    let record = CacheRecord::new(RecordKind::File, ttl::BULK_FILE, large.clone());
    l2.put("path/to/log", RecordKind::File, &record).unwrap();

    let got = l2.get("path/to/log", RecordKind::File).unwrap().unwrap();
    assert_eq!(got.payload, large);
}

#[test]
fn l2_put_batch() {
    let dir = tempfile::tempdir().unwrap();
    let l2 = BrowseCacheL2::open(&dir.path().join("browse.redb")).unwrap();

    let records = vec![
        ("a/title".to_string(), RecordKind::File,
         CacheRecord::new(RecordKind::File, ttl::PROJECTED_FILE, b"hello\n".to_vec())),
        ("a/body".to_string(), RecordKind::File,
         CacheRecord::new(RecordKind::File, ttl::PROJECTED_FILE, b"world\n".to_vec())),
        ("a".to_string(), RecordKind::Attr,
         CacheRecord::new(RecordKind::Attr, ttl::ATTR, vec![0, 0, 0, 0, 0, 0, 0, 0, 0])),
    ];
    l2.put_batch(&records).unwrap();

    assert!(l2.get("a/title", RecordKind::File).unwrap().is_some());
    assert!(l2.get("a/body", RecordKind::File).unwrap().is_some());
    assert!(l2.get("a", RecordKind::Attr).unwrap().is_some());
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p omnifs-host --test cache_l2_test`
Expected: Compilation error, `BrowseCacheL2` struct not defined.

- [ ] **Step 5: Implement BrowseCacheL2**

Create `crates/host/src/cache/l2.rs`:

```rust
//! L2 browse cache: durable, path-keyed, per-provider-instance redb database.

use crate::cache::{CacheRecord, RecordKind, L2_BULK_THRESHOLD};
use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;

const METADATA_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata");
const CONTENT_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("content");
const BULK_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("bulk");

pub struct BrowseCacheL2 {
    db: Database,
}

impl BrowseCacheL2 {
    pub fn open(path: &Path) -> Result<Self, redb::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let db = Database::create(path)?;
        // Ensure tables exist.
        let txn = db.begin_write()?;
        {
            let _ = txn.open_table(METADATA_TABLE)?;
            let _ = txn.open_table(CONTENT_TABLE)?;
            let _ = txn.open_table(BULK_TABLE)?;
        }
        txn.commit()?;
        Ok(Self { db })
    }

    pub fn get(&self, path: &str, kind: RecordKind) -> Result<Option<CacheRecord>, redb::Error> {
        let txn = self.db.begin_read()?;
        let key = make_key(path, kind);

        // For File records, check content first, then bulk.
        if kind == RecordKind::File {
            if let Some(record) = self.read_from_table(&txn, CONTENT_TABLE, &key)? {
                return Ok(Some(record));
            }
            return self.read_from_table(&txn, BULK_TABLE, &key);
        }

        self.read_from_table(&txn, METADATA_TABLE, &key)
    }

    pub fn put(
        &self,
        path: &str,
        kind: RecordKind,
        record: &CacheRecord,
    ) -> Result<(), redb::Error> {
        let txn = self.db.begin_write()?;
        let key = make_key(path, kind);
        let bytes = record.serialize();
        let target = self.table_for(kind, record.payload.len());
        {
            let mut table = txn.open_table(target)?;
            table.insert(key.as_str(), bytes.as_slice())?;
        }
        // Remove stale copy from the other file table if the record
        // crossed the bulk threshold since last write.
        if kind == RecordKind::File {
            let other = if target == CONTENT_TABLE { BULK_TABLE } else { CONTENT_TABLE };
            let mut other_table = txn.open_table(other)?;
            other_table.remove(key.as_str())?;
        }
        txn.commit()
    }

    pub fn put_batch(
        &self,
        records: &[(String, RecordKind, CacheRecord)],
    ) -> Result<(), redb::Error> {
        let txn = self.db.begin_write()?;
        {
            let mut meta = txn.open_table(METADATA_TABLE)?;
            let mut content = txn.open_table(CONTENT_TABLE)?;
            let mut bulk = txn.open_table(BULK_TABLE)?;
            for (path, kind, record) in records {
                let key = make_key(path, *kind);
                let bytes = record.serialize();
                let is_bulk = record.payload.len() >= L2_BULK_THRESHOLD;
                match (*kind, is_bulk) {
                    (RecordKind::File, true) => {
                        bulk.insert(key.as_str(), bytes.as_slice())?;
                        content.remove(key.as_str())?; // clear stale small copy
                    }
                    (RecordKind::File, false) => {
                        content.insert(key.as_str(), bytes.as_slice())?;
                        bulk.remove(key.as_str())?; // clear stale large copy
                    }
                    _ => { meta.insert(key.as_str(), bytes.as_slice())?; }
                };
            }
        }
        txn.commit()
    }

    fn read_from_table(
        &self,
        txn: &redb::ReadTransaction,
        table_def: TableDefinition<&str, &[u8]>,
        key: &str,
    ) -> Result<Option<CacheRecord>, redb::Error> {
        let table = txn.open_table(table_def)?;
        let Some(value) = table.get(key)? else {
            return Ok(None);
        };
        let Some(record) = CacheRecord::deserialize(value.value()) else {
            return Ok(None); // corrupt or unknown schema version; treat as miss
        };
        if record.is_expired() {
            return Ok(None); // lazy expiry
        }
        Ok(Some(record))
    }

    fn table_for(
        &self,
        kind: RecordKind,
        payload_len: usize,
    ) -> TableDefinition<'static, &'static str, &'static [u8]> {
        match kind {
            RecordKind::File if payload_len >= L2_BULK_THRESHOLD => BULK_TABLE,
            RecordKind::File => CONTENT_TABLE,
            _ => METADATA_TABLE,
        }
    }
}

fn make_key(path: &str, kind: RecordKind) -> String {
    let prefix = match kind {
        RecordKind::Lookup => 'L',
        RecordKind::Attr => 'A',
        RecordKind::Dirents => 'D',
        RecordKind::File => 'F',
    };
    format!("{prefix}:{path}")
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p omnifs-host --test cache_l2_test`
Expected: All 6 tests pass.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/host/Cargo.toml crates/host/src/cache/mod.rs crates/host/src/cache/l2.rs crates/host/tests/cache_l2_test.rs
git commit -m "feat(cache): add L2 browse cache backed by redb

BrowseCacheL2 wraps a redb database with three tables: metadata
(lookup/attr/dirents), content (files <64 KiB), and bulk (files >=64 KiB).
Lazy expiry on read; single-transaction batch writes."
```

---

### Task 4: L0 browse cache (moka)

**Files:**
- Modify: `Cargo.toml` (workspace deps)
- Modify: `crates/host/Cargo.toml`
- Modify: `crates/host/src/cache/mod.rs` (add `pub mod l0;`)
- Create: `crates/host/src/cache/l0.rs`
- Create: `crates/host/tests/cache_l0_test.rs`

- [ ] **Step 1: Add moka dependency**

In workspace `Cargo.toml`, add under `[workspace.dependencies]`:

```toml
moka = { version = "0.12", features = ["sync"] }
```

In `crates/host/Cargo.toml`, add under `[dependencies]`:

```toml
moka = { workspace = true }
```

- [ ] **Step 2: Register L0 submodule in cache/mod.rs**

Add `pub mod l0;` to `crates/host/src/cache/mod.rs` (after the existing `pub mod l2;`):

```rust
pub mod l0;
```

- [ ] **Step 3: Write the failing test for L0 operations**

Create `crates/host/tests/cache_l0_test.rs`:

```rust
use omnifs_host::cache::l0::{BrowseCacheL0, L0Key};
use omnifs_host::cache::{CacheRecord, RecordKind, ttl, L0_SKIP_THRESHOLD};

#[test]
fn l0_put_get() {
    let l0 = BrowseCacheL0::new();
    let key = L0Key::new(100, RecordKind::Attr, None);
    let record = CacheRecord::new(RecordKind::Attr, ttl::ATTR, vec![1, 0, 0, 0, 0, 0, 0, 0, 42]);
    l0.put(key.clone(), record.clone());

    let got = l0.get(&key);
    assert!(got.is_some());
    assert_eq!(got.unwrap().payload, vec![1, 0, 0, 0, 0, 0, 0, 0, 42]);
}

#[test]
fn l0_get_miss() {
    let l0 = BrowseCacheL0::new();
    let key = L0Key::new(999, RecordKind::File, None);
    assert!(l0.get(&key).is_none());
}

#[test]
fn l0_lookup_with_aux() {
    let l0 = BrowseCacheL0::new();
    let key = L0Key::new(10, RecordKind::Lookup, Some("title".to_string()));
    let record = CacheRecord::new(RecordKind::Lookup, ttl::LOOKUP_POSITIVE, vec![1, 1, 0, 0, 0, 0, 0, 0, 0, 42]);
    l0.put(key.clone(), record);

    assert!(l0.get(&key).is_some());

    // Different aux should miss
    let other_key = L0Key::new(10, RecordKind::Lookup, Some("body".to_string()));
    assert!(l0.get(&other_key).is_none());
}

#[test]
fn l0_skips_oversized_records() {
    let l0 = BrowseCacheL0::new();
    let key = L0Key::new(1, RecordKind::File, None);
    let big_payload = vec![0u8; L0_SKIP_THRESHOLD + 1];
    let record = CacheRecord::new(RecordKind::File, ttl::PROJECTED_FILE, big_payload);

    l0.put(key.clone(), record);
    // Oversized records are silently skipped
    assert!(l0.get(&key).is_none());
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p omnifs-host --test cache_l0_test`
Expected: Compilation error, `BrowseCacheL0` not defined.

- [ ] **Step 5: Implement BrowseCacheL0**

Create `crates/host/src/cache/l0.rs`:

```rust
//! L0 browse cache: in-memory, inode-keyed, byte-weighted moka cache.

use crate::cache::{CacheRecord, RecordKind, L0_MAX_WEIGHT, L0_SKIP_THRESHOLD};
use moka::sync::Cache;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct L0Key {
    pub inode: u64,
    pub kind: RecordKind,
    pub aux: Option<String>,
}

impl L0Key {
    pub fn new(inode: u64, kind: RecordKind, aux: Option<String>) -> Self {
        Self { inode, kind, aux }
    }
}

pub struct BrowseCacheL0 {
    cache: Cache<L0Key, Arc<CacheRecord>>,
}

impl BrowseCacheL0 {
    pub fn new() -> Self {
        let cache = Cache::builder()
            .max_capacity(L0_MAX_WEIGHT)
            .weigher(|key: &L0Key, value: &Arc<CacheRecord>| -> u32 {
                let key_size = 8 + 1 + key.aux.as_ref().map_or(0, |s| s.len());
                let val_size = 18 + value.payload.len();
                (key_size + val_size).try_into().unwrap_or(u32::MAX)
            })
            .time_to_idle(Duration::from_secs(600))
            .build();
        Self { cache }
    }

    pub fn get(&self, key: &L0Key) -> Option<Arc<CacheRecord>> {
        let record = self.cache.get(key)?;
        if record.is_expired() {
            self.cache.invalidate(key);
            return None;
        }
        Some(record)
    }

    pub fn put(&self, key: L0Key, record: CacheRecord) {
        if record.payload.len() > L0_SKIP_THRESHOLD {
            return;
        }
        self.cache.insert(key, Arc::new(record));
    }

    pub fn invalidate(&self, key: &L0Key) {
        self.cache.invalidate(key);
    }
}

impl Default for BrowseCacheL0 {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p omnifs-host --test cache_l0_test`
Expected: All 4 tests pass.

- [ ] **Step 7: Verify full test suite**

Run: `cargo test -p omnifs-host`
Expected: All tests pass (existing + new cache tests).

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml crates/host/Cargo.toml crates/host/src/cache/mod.rs crates/host/src/cache/l0.rs crates/host/tests/cache_l0_test.rs
git commit -m "feat(cache): add L0 browse cache backed by moka

BrowseCacheL0 wraps moka::sync::Cache with byte-weighted eviction
(32 MiB cap), L0Key with (inode, kind, aux) composite key, and
skip threshold for records above 256 KiB."
```

---

### Task 5: Wire L2 into EffectRuntime

**Files:**
- Modify: `crates/host/src/runtime/mod.rs:31-38` (EffectRuntime struct)
- Modify: `crates/host/src/runtime/mod.rs:83-138` (EffectRuntime::new)
- Modify: `crates/host/src/registry.rs:29-99` (ProviderRegistry::load)
- Modify: `crates/host/src/registry.rs:100-122` (load_instance)
- Modify: `crates/cli/src/main.rs:91` (registry load call)

- [ ] **Step 1: Add cache_dir parameter to ProviderRegistry::load**

In `crates/host/src/registry.rs`, change the `load` signature to accept `cache_dir`:

```rust
pub fn load(
    config_dir: &Path,
    plugin_dir: &Path,
    cloner: &Arc<GitCloner>,
    cache_dir: &Path,
) -> Result<Self, RegistryError> {
```

Thread `cache_dir` into `load_instance`:

```rust
match Self::load_instance(&engine, &path, plugin_dir, cloner, cache_dir) {
```

Update `load_instance` to accept and forward `cache_dir`:

```rust
fn load_instance(
    engine: &wasmtime::Engine,
    config_path: &Path,
    plugin_dir: &Path,
    cloner: &Arc<GitCloner>,
    cache_dir: &Path,
) -> Result<(String, bool, EffectRuntime), RegistryError> {
    // ... existing code ...
    let runtime = EffectRuntime::new(engine, &wasm_path, &config, cloner.clone(), cache_dir, &config.mount)
        .map_err(|e| RegistryError::RuntimeError(e.to_string()))?;
    Ok((config.mount.clone(), is_root, runtime))
}
```

- [ ] **Step 2: Add BrowseCacheL2 to EffectRuntime**

In `crates/host/src/runtime/mod.rs`, add the import and field:

```rust
use crate::cache::l2::BrowseCacheL2;
use crate::cache::{CacheRecord, RecordKind};
```

Add to the `EffectRuntime` struct:

```rust
pub struct EffectRuntime {
    store: Mutex<wasmtime::Store<HostState>>,
    bindings: Provider,
    correlations: CorrelationTracker,
    http: HttpExecutor,
    kv: MemoryKvExecutor,
    git: git::GitExecutor,
    l2: Option<BrowseCacheL2>,
}
```

Update `EffectRuntime::new` to accept `cache_dir` and `mount_name`, then open L2:

```rust
pub fn new(
    engine: &wasmtime::Engine,
    wasm_path: &Path,
    config: &InstanceConfig,
    cloner: Arc<GitCloner>,
    cache_dir: &Path,
    mount_name: &str,
) -> Result<Self, RuntimeError> {
    // ... existing code up to git executor ...

    let l2 = {
        let db_path = cache_dir.join("providers").join(mount_name).join("browse.redb");
        match BrowseCacheL2::open(&db_path) {
            Ok(cache) => Some(cache),
            Err(e) => {
                tracing::warn!(mount = mount_name, error = %e, "failed to open L2 browse cache");
                None
            }
        }
    };

    Ok(Self {
        store: Mutex::new(store),
        bindings,
        correlations: CorrelationTracker::new(),
        http: HttpExecutor::new(auth, capability),
        kv: MemoryKvExecutor::new(),
        git,
        l2,
    })
}
```

- [ ] **Step 3: Add public cache methods to EffectRuntime**

Add these methods to `impl EffectRuntime` in `crates/host/src/runtime/mod.rs`:

```rust
pub fn cache_get(&self, path: &str, kind: RecordKind) -> Option<CacheRecord> {
    self.l2.as_ref()?.get(path, kind).ok().flatten()
}

pub fn cache_put(&self, path: &str, kind: RecordKind, record: &CacheRecord) {
    if let Some(ref l2) = self.l2 {
        if let Err(e) = l2.put(path, kind, record) {
            tracing::debug!(path, error = %e, "L2 cache put failed");
        }
    }
}

pub fn cache_put_batch(&self, records: &[(String, RecordKind, CacheRecord)]) {
    if let Some(ref l2) = self.l2 {
        if let Err(e) = l2.put_batch(records) {
            tracing::debug!(error = %e, "L2 cache batch put failed");
        }
    }
}
```

> **Note:** `cache_delete_prefix` is added later in Task 11, alongside the L2
> `delete_prefix` implementation and the invalidation queue. Do not add it here.

- [ ] **Step 4: Update CLI to pass cache_dir to registry**

In `crates/cli/src/main.rs`, update the registry load call (~line 91):

```rust
let registry = ProviderRegistry::load(&config_path, &plugin_dir, &cloner, cloner.cache_dir())?;
```

(`cache_path` was moved into `GitCloner::new(cache_path)` on line 82, so use
`cloner.cache_dir()` which returns a `&Path` reference to the same directory.)

- [ ] **Step 5: Build and verify**

Run: `cargo build --workspace`
Expected: Clean build. Existing tests still pass.

Run: `cargo test -p omnifs-host`
Expected: All tests pass. (Existing runtime_test.rs and git_executor_test.rs call `EffectRuntime::new` in tests; update those call sites to pass dummy `cache_dir` and `mount_name`.)

**Note:** Update `crates/host/tests/runtime_test.rs` and `crates/host/tests/git_executor_test.rs` to pass the new parameters:

```rust
let cloner = Arc::new(GitCloner::new(PathBuf::from("/tmp/omnifs-test-cache")));
let runtime = EffectRuntime::new(&engine, &wasm_path, &config, cloner, Path::new("/tmp/omnifs-test-l2"), "test-mount")
```

- [ ] **Step 6: Commit**

```bash
git add crates/host/src/runtime/mod.rs crates/host/src/registry.rs crates/cli/src/main.rs crates/host/tests/
git commit -m "feat(cache): wire L2 browse cache into EffectRuntime

EffectRuntime now opens a per-provider BrowseCacheL2 redb database at
\${cache_dir}/providers/\${mount_name}/browse.redb. Exposes cache_get,
cache_put, cache_put_batch, and cache_delete_prefix public methods for
FuseFs and provider-driven invalidation to use."
```

---

### Task 6: Wire L0 into FuseFs

**Files:**
- Modify: `crates/host/src/fuse/mod.rs:57-93` (FuseFs struct and new)

- [ ] **Step 1: Add L0 caches to FuseFs**

In `crates/host/src/fuse/mod.rs`, add imports:

```rust
use crate::cache::l0::{BrowseCacheL0, L0Key};
use crate::cache::{CacheRecord, RecordKind};
use std::collections::HashMap;
```

Add field to `FuseFs` struct:

```rust
pub struct FuseFs {
    rt: Handle,
    registry: Arc<ProviderRegistry>,
    inodes: DashMap<u64, InodeEntry>,
    path_to_inode: DashMap<(String, String), u64>,
    next_ino: AtomicU64,
    dir_snapshots: DashMap<u64, Vec<(u64, String, EntryKind)>>,
    next_fh: AtomicU64,
    file_cache: DashMap<u64, Vec<u8>>,
    l0_caches: DashMap<String, BrowseCacheL0>,
}
```

Update `FuseFs::new` to initialize the map:

```rust
Self {
    rt,
    registry,
    inodes,
    path_to_inode: DashMap::new(),
    next_ino: AtomicU64::new(2),
    dir_snapshots: DashMap::new(),
    next_fh: AtomicU64::new(1),
    file_cache: DashMap::new(),
    l0_caches: DashMap::new(),
}
```

- [ ] **Step 2: Add L0 helper methods**

Add to `impl FuseFs` (in `crates/host/src/fuse/mod.rs`, after `runtime_for_mount`):

```rust
/// Get or lazily create the L0 cache for a mount.
fn l0_for_mount(&self, mount: &str) -> dashmap::mapref::one::Ref<'_, String, BrowseCacheL0> {
    self.l0_caches
        .entry(mount.to_string())
        .or_insert_with(BrowseCacheL0::new);
    self.l0_caches.get(mount).unwrap()
}

fn l0_get(&self, mount: &str, inode: u64, kind: RecordKind, aux: Option<String>) -> Option<std::sync::Arc<CacheRecord>> {
    let l0 = self.l0_for_mount(mount);
    l0.get(&L0Key::new(inode, kind, aux))
}

fn l0_put(&self, mount: &str, inode: u64, kind: RecordKind, aux: Option<String>, record: CacheRecord) {
    let l0 = self.l0_for_mount(mount);
    l0.put(L0Key::new(inode, kind, aux), record);
}
```

- [ ] **Step 3: Build and verify**

Run: `cargo build --workspace`
Expected: Clean build. The L0 caches are created but not yet consulted in FUSE operations.

- [ ] **Step 4: Commit**

```bash
git add crates/host/src/fuse/mod.rs
git commit -m "feat(cache): wire L0 browse cache into FuseFs

FuseFs now holds a per-mount DashMap of BrowseCacheL0 instances,
lazily created on first access. L0 helper methods for get/put
with composite (inode, kind, aux) keys."
```

---

### Task 7: FUSE read path integration

**Files:**
- Modify: `crates/host/src/fuse/mod.rs:107-236` (lookup)
- Modify: `crates/host/src/fuse/mod.rs:269-412` (opendir)
- Modify: `crates/host/src/fuse/mod.rs:454-541` (read)

- [ ] **Step 1: Add L0/L2 checks to lookup**

In the `lookup` method of `crates/host/src/fuse/mod.rs`, after the `path_to_inode` check (line 177) and before the `runtime_for_mount` call (line 179), insert L0 and L2 checks. Replace the block from `let Some(runtime)` through the end of the provider match with:

```rust
// --- L0/L2 cache path (only for provider-delegated lookups) ---

// L0: check cached lookup by parent inode + child name
if let Some(record) = self.l0_get(&mount, parent.0, RecordKind::Lookup, Some(name_str.to_string())) {
    if let Some(lookup) = crate::cache::LookupPayload::deserialize(&record.payload) {
        match lookup {
            crate::cache::LookupPayload::Negative => {
                reply.error(Errno::ENOENT);
                return;
            }
            crate::cache::LookupPayload::Positive { kind, size } => {
                let ek = entry_kind_from_cache(kind);
                let ino = self.get_or_alloc_ino(&mount, &child_path, ek, size);
                let attr = match ek {
                    EntryKind::Directory => self.dir_attr(ino),
                    EntryKind::File => self.file_attr(ino, size),
                };
                reply.entry(&TTL, &attr, Generation(0));
                return;
            }
        }
    }
}

// L2: check cached lookup by path (through runtime)
let runtime = match self.runtime_for_mount(&mount) {
    Some(rt) => rt,
    None => {
        reply.error(Errno::ENOENT);
        return;
    }
};

if let Some(record) = runtime.cache_get(&child_path, RecordKind::Lookup) {
    if let Some(lookup) = crate::cache::LookupPayload::deserialize(&record.payload) {
        // Promote to L0
        self.l0_put(&mount, parent.0, RecordKind::Lookup, Some(name_str.to_string()), record.clone());
        match lookup {
            crate::cache::LookupPayload::Negative => {
                reply.error(Errno::ENOENT);
                return;
            }
            crate::cache::LookupPayload::Positive { kind, size } => {
                let ek = entry_kind_from_cache(kind);
                let ino = self.get_or_alloc_ino(&mount, &child_path, ek, size);
                let attr = match ek {
                    EntryKind::Directory => self.dir_attr(ino),
                    EntryKind::File => self.file_attr(ino, size),
                };
                reply.entry(&TTL, &attr, Generation(0));
                return;
            }
        }
    }
}

// Provider call (existing code, but now also writes through to L2 + L0)
match self.rt.block_on(runtime.call_resolve_entry(&parent_path, name_str)) {
    // ... existing DisownedTree arm unchanged ...
    Ok(ActionResult::DirEntryOption(Some(entry))) => {
        let size = entry.size.unwrap_or(0);
        let ino = self.get_or_alloc_ino(&mount, &child_path, entry.kind, size);

        // Write-through to L2 and L0
        let kind_cache = cache_entry_kind(entry.kind);
        let lookup_payload = crate::cache::LookupPayload::Positive { kind: kind_cache, size };
        let lookup_record = CacheRecord::new(RecordKind::Lookup, crate::cache::ttl::LOOKUP_POSITIVE, lookup_payload.serialize());
        runtime.cache_put(&child_path, RecordKind::Lookup, &lookup_record);
        self.l0_put(&mount, parent.0, RecordKind::Lookup, Some(name_str.to_string()), lookup_record);

        let attr = match entry.kind {
            EntryKind::Directory => self.dir_attr(ino),
            EntryKind::File => self.file_attr(ino, size),
        };
        reply.entry(&TTL, &attr, Generation(0));
    }
    Ok(ActionResult::DirEntryOption(None)) => {
        reply.error(Errno::ENOENT);
    }
    Ok(_) | Err(_) => {
        reply.error(Errno::EIO);
    }
}
```

Add these conversion helpers at the bottom of `crates/host/src/fuse/mod.rs` (outside the `Filesystem` impl):

```rust
fn entry_kind_from_cache(kind: crate::cache::EntryKindCache) -> EntryKind {
    match kind {
        crate::cache::EntryKindCache::Directory => EntryKind::Directory,
        crate::cache::EntryKindCache::File => EntryKind::File,
    }
}

fn cache_entry_kind(kind: EntryKind) -> crate::cache::EntryKindCache {
    match kind {
        EntryKind::Directory => crate::cache::EntryKindCache::Directory,
        EntryKind::File => crate::cache::EntryKindCache::File,
    }
}
```

- [ ] **Step 2: Add L0/L2 checks to opendir**

In the `opendir` method, after extracting `mount`, `path`, `real` (line 296) and before the `runtime_for_mount` call, insert L0/L2 checks. The cache path applies only when `real` is None and this is a provider-delegated listing:

```rust
// L0: check cached dirents by inode
if let Some(record) = self.l0_get(&mount, ino.0, RecordKind::Dirents, None) {
    if let Some(dirents) = crate::cache::DirentsPayload::deserialize(&record.payload) {
        let mut snapshot = Vec::with_capacity(dirents.entries.len());
        for e in &dirents.entries {
            let child_path = if path.is_empty() {
                e.name.clone()
            } else {
                format!("{path}/{}", e.name)
            };
            let ek = entry_kind_from_cache(e.kind);
            let child_ino = self.get_or_alloc_ino(&mount, &child_path, ek, e.size);
            snapshot.push((child_ino, e.name.clone(), ek));
        }
        self.dir_snapshots.insert(fh, snapshot);
        reply.opened(FuseFileHandle(fh), FopenFlags::empty());
        return;
    }
}

// L2: check cached dirents by path (through runtime)
if let Some(runtime) = self.runtime_for_mount(&mount) {
    if let Some(record) = runtime.cache_get(&path, RecordKind::Dirents) {
        if let Some(dirents) = crate::cache::DirentsPayload::deserialize(&record.payload) {
            // Promote to L0
            self.l0_put(&mount, ino.0, RecordKind::Dirents, None, record.clone());

            let mut snapshot = Vec::with_capacity(dirents.entries.len());
            for e in &dirents.entries {
                let child_path = if path.is_empty() {
                    e.name.clone()
                } else {
                    format!("{path}/{}", e.name)
                };
                let ek = entry_kind_from_cache(e.kind);
                let child_ino = self.get_or_alloc_ino(&mount, &child_path, ek, e.size);
                snapshot.push((child_ino, e.name.clone(), ek));
            }
            self.dir_snapshots.insert(fh, snapshot);
            reply.opened(FuseFileHandle(fh), FopenFlags::empty());
            return;
        }
    }
}
```

In the existing `ActionResult::DirEntries(dir_entries)` arm, after building the snapshot, add L2 + L0 write-through:

```rust
Ok(ActionResult::DirEntries(dir_entries)) => {
    let mut snapshot = Vec::with_capacity(dir_entries.len());
    let mut dirent_records = Vec::with_capacity(dir_entries.len());
    for e in &dir_entries {
        let child_path = if path.is_empty() {
            e.name.clone()
        } else {
            format!("{path}/{}", e.name)
        };
        let size = e.size.unwrap_or(0);
        let child_ino = self.get_or_alloc_ino(&mount, &child_path, e.kind, size);
        snapshot.push((child_ino, e.name.clone(), e.kind));
        dirent_records.push(crate::cache::DirentRecord {
            name: e.name.clone(),
            kind: cache_entry_kind(e.kind),
            size,
        });
    }

    // Write-through dirents to L2 + L0
    let dirents_payload = crate::cache::DirentsPayload { entries: dirent_records };
    let dirents_record = CacheRecord::new(
        RecordKind::Dirents,
        crate::cache::ttl::DIRENTS,
        dirents_payload.serialize(),
    );
    if let Some(runtime) = self.runtime_for_mount(&mount) {
        runtime.cache_put(&path, RecordKind::Dirents, &dirents_record);
    }
    self.l0_put(&mount, ino.0, RecordKind::Dirents, None, dirents_record);

    self.dir_snapshots.insert(fh, snapshot);
    reply.opened(FuseFileHandle(fh), FopenFlags::empty());
}
```

- [ ] **Step 3: Add L0/L2 checks to read**

In the `read` method, after the `file_cache` check (line 481) and before looking up the inode entry, insert L0/L2 checks. After extracting `mount`, `path`, `real` and before the passthrough check, add:

```rust
// L0: check cached file by inode
if let Some(record) = self.l0_get(&mount, ino.0, RecordKind::File, None) {
    let data = &record.payload;
    let start = offset as usize;
    let end = (start + size as usize).min(data.len());
    if start >= data.len() {
        reply.data(&[]);
    } else {
        reply.data(&data[start..end]);
    }
    self.file_cache.insert(fh.0, data.clone());
    return;
}

// L2: check cached file by path (only for non-passthrough)
if real.is_none() {
    if let Some(runtime) = self.runtime_for_mount(&mount) {
        if let Some(record) = runtime.cache_get(&path, RecordKind::File) {
            let data = record.payload.clone();
            // Promote to L0
            self.l0_put(&mount, ino.0, RecordKind::File, None, record.clone());
            let start = offset as usize;
            let end = (start + size as usize).min(data.len());
            if start >= data.len() {
                self.file_cache.insert(fh.0, data);
                reply.data(&[]);
            } else {
                reply.data(&data[start..end]);
                self.file_cache.insert(fh.0, data);
            }
            return;
        }
    }
}
```

In the existing `ActionResult::FileContent(data)` arm, add L2 + L0 write-through:

```rust
Ok(ActionResult::FileContent(data)) => {
    // Write-through to L2 + L0
    let ttl = if data.len() >= crate::cache::L2_BULK_THRESHOLD {
        crate::cache::ttl::BULK_FILE
    } else {
        crate::cache::ttl::PROJECTED_FILE
    };
    let file_record = CacheRecord::new(RecordKind::File, ttl, data.clone());
    if let Some(rt) = self.runtime_for_mount(&mount) {
        rt.cache_put(&path, RecordKind::File, &file_record);
    }
    self.l0_put(&mount, ino.0, RecordKind::File, None, file_record);

    let start = offset as usize;
    let end = (start + size as usize).min(data.len());
    if start >= data.len() {
        self.file_cache.insert(fh.0, data);
        reply.data(&[]);
    } else {
        reply.data(&data[start..end]);
        self.file_cache.insert(fh.0, data);
    }
}
```

- [ ] **Step 4: Build and verify**

Run: `cargo build --workspace`
Expected: Clean build.

Run: `cargo test -p omnifs-host`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/host/src/fuse/mod.rs
git commit -m "feat(cache): integrate L0/L2 into FUSE lookup, opendir, and read

Each FUSE operation now checks L0 (by inode) then L2 (by path)
before falling through to the provider. Provider results write
through to both tiers. Read path: file_cache -> L0 -> L2 -> provider."
```

---

### Task 8: Projected-files extraction in host

**Files:**
- Modify: `crates/host/src/runtime/mod.rs:196-211` (call_list_entries)

- [ ] **Step 1: Add projected-files extraction to call_list_entries**

In `crates/host/src/runtime/mod.rs`, modify `call_list_entries` to intercept the `DirEntries` result and extract projected files into L2 before returning:

```rust
pub async fn call_list_entries(
    &self,
    path: &str,
) -> Result<wit_types::ActionResult, RuntimeError> {
    let id = self.correlations.allocate();
    self.correlations.mark_pending(id, "list_entries".into());

    let response = {
        let mut store = self.store.lock();
        self.bindings
            .omnifs_provider_browse()
            .call_list_entries(&mut *store, id, path)?
    };

    let result = self.drive_effects(id, response).await?;

    // Intercept DirEntries to extract and cache projected files.
    if let wit_types::ActionResult::DirEntries(ref entries) = result {
        self.extract_projected_files(path, entries);
    }

    // Strip projected_files before returning to FUSE.
    Ok(self.strip_projected_files(result))
}
```

Add these private methods to `impl EffectRuntime`:

```rust
/// Extract projected files from DirEntries and batch-write to L2.
fn extract_projected_files(&self, parent_path: &str, entries: &[wit_types::DirEntry]) {
    use crate::cache::{
        AttrPayload, CacheRecord, DirentRecord, DirentsPayload, EntryKindCache,
        LookupPayload, RecordKind, ttl,
    };

    let mut batch = Vec::new();

    // Cache dirents for the parent directory.
    let dirent_records: Vec<DirentRecord> = entries
        .iter()
        .map(|e| DirentRecord {
            name: e.name.clone(),
            kind: match e.kind {
                wit_types::EntryKind::Directory => EntryKindCache::Directory,
                wit_types::EntryKind::File => EntryKindCache::File,
            },
            size: e.size.unwrap_or(0),
        })
        .collect();
    let dirents_payload = DirentsPayload { entries: dirent_records };
    batch.push((
        parent_path.to_string(),
        RecordKind::Dirents,
        CacheRecord::new(RecordKind::Dirents, ttl::DIRENTS, dirents_payload.serialize()),
    ));

    for entry in entries {
        let child_path = if parent_path.is_empty() {
            entry.name.clone()
        } else {
            format!("{parent_path}/{}", entry.name)
        };

        let kind_cache = match entry.kind {
            wit_types::EntryKind::Directory => EntryKindCache::Directory,
            wit_types::EntryKind::File => EntryKindCache::File,
        };
        let size = entry.size.unwrap_or(0);

        // Cache lookup record for child.
        let lookup = LookupPayload::Positive { kind: kind_cache, size };
        batch.push((
            child_path.clone(),
            RecordKind::Lookup,
            CacheRecord::new(RecordKind::Lookup, ttl::LOOKUP_POSITIVE, lookup.serialize()),
        ));

        // Cache attr record for child.
        let attr = AttrPayload { kind: kind_cache, size };
        batch.push((
            child_path.clone(),
            RecordKind::Attr,
            CacheRecord::new(RecordKind::Attr, ttl::ATTR, attr.serialize()),
        ));

        // Cache projected files.
        if let Some(ref projected) = entry.projected_files {
            for pf in projected {
                let file_path = format!("{child_path}/{}", pf.name);
                let file_size = pf.content.len() as u64;

                // File content record.
                batch.push((
                    file_path.clone(),
                    RecordKind::File,
                    CacheRecord::new(RecordKind::File, ttl::PROJECTED_FILE, pf.content.clone()),
                ));

                // Lookup record for the projected file.
                let pf_lookup = LookupPayload::Positive {
                    kind: EntryKindCache::File,
                    size: file_size,
                };
                batch.push((
                    file_path.clone(),
                    RecordKind::Lookup,
                    CacheRecord::new(RecordKind::Lookup, ttl::LOOKUP_POSITIVE, pf_lookup.serialize()),
                ));

                // Attr record for the projected file.
                let pf_attr = AttrPayload {
                    kind: EntryKindCache::File,
                    size: file_size,
                };
                batch.push((
                    file_path,
                    RecordKind::Attr,
                    CacheRecord::new(RecordKind::Attr, ttl::ATTR, pf_attr.serialize()),
                ));
            }
        }
    }

    if !batch.is_empty() {
        self.cache_put_batch(&batch);
    }
}

/// Strip projected_files from DirEntries before handing to FUSE.
fn strip_projected_files(&self, result: wit_types::ActionResult) -> wit_types::ActionResult {
    if let wit_types::ActionResult::DirEntries(entries) = result {
        let stripped: Vec<wit_types::DirEntry> = entries
            .into_iter()
            .map(|mut e| {
                e.projected_files = None;
                e
            })
            .collect();
        wit_types::ActionResult::DirEntries(stripped)
    } else {
        result
    }
}
```

- [ ] **Step 2: Build and verify**

Run: `cargo build --workspace`
Expected: Clean build.

Run: `cargo test -p omnifs-host`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/host/src/runtime/mod.rs
git commit -m "feat(cache): extract projected files from DirEntries into L2

call_list_entries now intercepts DirEntries results, extracts dirents,
lookup, attr, and projected-file records into a single L2 batch write,
then strips projected_files before returning to FUSE."
```

---

### Task 9: GitHub provider projected-files population

**Files:**
- Modify: `providers/github/src/browse/resources.rs:442-466` (finalize_search_results)
- Modify: `providers/github/src/browse/files.rs` (extract_str reuse)

- [ ] **Step 1: Populate projected-files in finalize_search_results**

In `providers/github/src/browse/resources.rs`, update `finalize_search_results` to attach projected files to each issue/PR directory entry:

```rust
pub fn finalize_search_results(path: &str, items: &[serde_json::Value]) -> ProviderResponse {
    let Some(FsPath::ResourceFilter {
        owner, repo, kind, ..
    }) = FsPath::parse(path)
    else {
        return err("invalid resource filter path");
    };
    let api_resource = kind.api_path();

    let entries = items
        .iter()
        .filter_map(|item| {
            let number = item.get("number")?.as_u64()?;
            let cache_key = format!("{owner}/{repo}/{api_resource}/{number}");
            let item_bytes = serde_json::to_vec(item).ok()?;
            let _ = with_state(|state| state.cache.set(cache_key, item_bytes));

            // Build projected files from the search result JSON.
            let projected = build_projected_files(item);

            Some(DirEntry {
                name: number.to_string(),
                kind: EntryKind::Directory,
                size: None,
                projected_files: Some(projected),
            })
        })
        .collect();
    ProviderResponse::Done(ActionResult::DirEntries(entries))
}
```

Add the helper function in the same file:

```rust
fn build_projected_files(item: &serde_json::Value) -> Vec<ProjectedFile> {
    let title = item
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
        + "\n";
    let body = item
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
        + "\n";
    let state = item
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
        + "\n";
    let user = item
        .get("user")
        .and_then(|u| u.get("login"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
        + "\n";

    vec![
        ProjectedFile { name: "title".to_string(), content: title.into_bytes() },
        ProjectedFile { name: "body".to_string(), content: body.into_bytes() },
        ProjectedFile { name: "state".to_string(), content: state.into_bytes() },
        ProjectedFile { name: "user".to_string(), content: user.into_bytes() },
    ]
}
```

Add `ProjectedFile` to the imports at the top of `resources.rs`:

```rust
use crate::omnifs::provider::types::*;
```

(This already imports everything including the new `ProjectedFile` type.)

- [ ] **Step 2: Also populate projected-files for single-page results**

In `resume_list_first_page` (same file), the single-page path calls `finalize_search_results` which already handles it. The action runs path does NOT need projected files (those are separate). Verify that the code path for `FsPath::ResourceFilter` with `page_count <= 1` calls `finalize_search_results`.

- [ ] **Step 3: Build the provider**

Run: `just build-providers`
Expected: Provider components build cleanly, including `omnifs-provider-github`.

- [ ] **Step 4: Build full workspace**

Run: `cargo build --workspace`
Expected: Clean build.

- [ ] **Step 5: Commit**

```bash
git add providers/github/src/browse/resources.rs
git commit -m "feat(github): populate projected-files for issue/PR listings

finalize_search_results now attaches title, body, state, and user as
projected files on each issue/PR directory entry. The host extracts
these into L2 so subsequent file reads bypass the provider."
```

---

### Task 10: Negative caching

**Files:**
- Modify: `crates/host/src/fuse/mod.rs` (lookup method)

- [ ] **Step 1: Add dirents-implied negative check**

In the `lookup` method of `crates/host/src/fuse/mod.rs`, BEFORE the L0 lookup check (added in Task 7), add a dirents-implied negative check:

```rust
// Dirents-implied negative: if parent dirents are cached and child is absent, ENOENT.
if let Some(record) = self.l0_get(&mount, parent.0, RecordKind::Dirents, None) {
    if let Some(dirents) = crate::cache::DirentsPayload::deserialize(&record.payload) {
        if !dirents.entries.iter().any(|e| e.name == name_str) {
            reply.error(Errno::ENOENT);
            return;
        }
    }
}
```

- [ ] **Step 2: Cache negative lookup records on provider miss**

In the `lookup` method, update the `DirEntryOption(None)` arm to cache a negative:

```rust
Ok(ActionResult::DirEntryOption(None)) => {
    // Cache negative lookup.
    let neg = crate::cache::LookupPayload::Negative;
    let neg_record = CacheRecord::new(
        RecordKind::Lookup,
        crate::cache::ttl::LOOKUP_NEGATIVE,
        neg.serialize(),
    );
    runtime.cache_put(&child_path, RecordKind::Lookup, &neg_record);
    self.l0_put(&mount, parent.0, RecordKind::Lookup, Some(name_str.to_string()), neg_record);

    reply.error(Errno::ENOENT);
}
```

- [ ] **Step 3: Build and verify**

Run: `cargo build --workspace`
Expected: Clean build.

- [ ] **Step 4: Commit**

```bash
git add crates/host/src/fuse/mod.rs
git commit -m "feat(cache): add negative caching for lookup misses

Dirents-implied negatives return ENOENT when the parent directory
listing is cached and the child name is absent. Explicit negative
lookup records with 30s TTL are cached on provider miss."
```

---

### Task 11: Provider-driven prefix invalidation

**Files:**
- Modify: `crates/host/src/cache/l2.rs` (add `delete_prefix`)
- Modify: `crates/host/src/runtime/mod.rs` (add `cache_delete_prefix`, invalidation queue, handle new effect)
- Modify: `crates/host/src/fuse/mod.rs` (drain invalidations and evict matching L0 entries)
- Modify: `providers/github/src/browse/events.rs` (emit host cache invalidation effects)

> **Note:** The WIT changes for `cache-invalidate-prefix` and `cache-ok` were
> already made in Task 1 Step 1. This task implements the host and provider
> sides of the new effect.

- [ ] **Step 1: Add prefix deletion to BrowseCacheL2**

In `crates/host/src/cache/l2.rs`, add this method to `impl BrowseCacheL2`:

```rust
/// Delete all records whose logical path starts with `prefix`.
///
/// The stored key format is `{kind_char}:{path}`, so we scan each table
/// for keys matching `L:{prefix}`, `A:{prefix}`, `D:{prefix}`, `F:{prefix}`
/// using redb's ordered range iteration.
pub fn delete_prefix(&self, prefix: &str) -> Result<usize, redb::Error> {
    let txn = self.db.begin_write()?;
    let mut deleted = 0;
    let tables = [METADATA_TABLE, CONTENT_TABLE, BULK_TABLE];
    let kind_chars = ['L', 'A', 'D', 'F'];

    for table_def in tables {
        let mut table = txn.open_table(table_def)?;
        let mut to_delete = Vec::new();
        for ch in &kind_chars {
            let scan_prefix = format!("{ch}:{prefix}");
            // Range from scan_prefix.. to collect matching keys.
            // redb range is inclusive-start, exclusive-end. We use the
            // prefix incremented by one char as the upper bound.
            let range_end = {
                let mut end = scan_prefix.clone();
                // Append a high char to create an exclusive upper bound.
                end.push(char::MAX);
                end
            };
            let range = table.range::<&str>(scan_prefix.as_str()..range_end.as_str())?;
            for entry in range {
                let entry = entry?;
                to_delete.push(entry.0.value().to_string());
            }
        }
        for key in &to_delete {
            table.remove(key.as_str())?;
            deleted += 1;
        }
    }
    txn.commit()?;
    Ok(deleted)
}
```

- [ ] **Step 2: Add invalidation queue and cache_delete_prefix to EffectRuntime**

In `crates/host/src/runtime/mod.rs`, add the import and field:

```rust
use parking_lot::Mutex; // already imported
use crate::cache::l2::BrowseCacheL2;
use crate::cache::{CacheRecord, RecordKind};
```

Add a field to `EffectRuntime`:

```rust
pub struct EffectRuntime {
    store: Mutex<wasmtime::Store<HostState>>,
    bindings: Provider,
    correlations: CorrelationTracker,
    http: HttpExecutor,
    kv: MemoryKvExecutor,
    git: git::GitExecutor,
    l2: Option<BrowseCacheL2>,
    invalidated_prefixes: Mutex<Vec<String>>,
}
```

Initialize in `EffectRuntime::new`:

```rust
Ok(Self {
    store: Mutex::new(store),
    bindings,
    correlations: CorrelationTracker::new(),
    http: HttpExecutor::new(auth, capability),
    kv: MemoryKvExecutor::new(),
    git,
    l2,
    invalidated_prefixes: Mutex::new(Vec::new()),
})
```

Add these methods to `impl EffectRuntime`:

```rust
pub fn cache_delete_prefix(&self, prefix: &str) {
    if let Some(ref l2) = self.l2 {
        if let Err(e) = l2.delete_prefix(prefix) {
            tracing::debug!(prefix, error = %e, "L2 cache prefix delete failed");
        }
    }
}

/// Drain and return pending invalidated prefixes. Called by FuseFs
/// before checking L0 to ensure stale entries are evicted.
pub fn drain_invalidated_prefixes(&self) -> Vec<String> {
    let mut prefixes = self.invalidated_prefixes.lock();
    std::mem::take(&mut *prefixes)
}
```

- [ ] **Step 3: Handle the cache-invalidate-prefix effect in execute_single_effect**

In `crates/host/src/runtime/mod.rs`, update `execute_single_effect` to handle the new variant. Replace the catch-all `_ =>` arm:

```rust
wit_types::SingleEffect::CacheInvalidatePrefix(req) => {
    self.cache_delete_prefix(&req.prefix);
    self.invalidated_prefixes.lock().push(req.prefix.clone());
    wit_types::SingleEffectResult::CacheOk
}
, // keep the existing catch-all after this arm:
_ => wit_types::SingleEffectResult::EffectError(wit_types::EffectError {
    kind: wit_types::ErrorKind::Internal,
    message: "effect type not yet implemented".to_string(),
    retryable: false,
}),
```

- [ ] **Step 4: Evict L0 entries when invalidations arrive**

In `crates/host/src/fuse/mod.rs`, add a helper to `impl FuseFs`:

```rust
/// Drain pending invalidation prefixes from the runtime and evict
/// matching L0 cache entries. Coarse: iterates all inodes for the mount
/// and evicts any whose path starts with an invalidated prefix.
fn drain_and_evict_l0(&self, mount: &str) {
    let Some(runtime) = self.runtime_for_mount(mount) else {
        return;
    };
    let prefixes = runtime.drain_invalidated_prefixes();
    if prefixes.is_empty() {
        return;
    }
    let Some(l0) = self.l0_caches.get(mount) else {
        return;
    };
    // Collect (inode, path) pairs for this mount, then evict matches.
    let mount_inodes: Vec<(u64, String)> = self
        .inodes
        .iter()
        .filter(|entry| entry.value().mount == mount)
        .map(|entry| (*entry.key(), entry.value().path.clone()))
        .collect();

    for (ino, path) in &mount_inodes {
        for prefix in &prefixes {
            if path == prefix || path.starts_with(prefix) {
                // Evict all record kinds for this inode (aux=None).
                for kind in [RecordKind::Lookup, RecordKind::Attr, RecordKind::Dirents, RecordKind::File] {
                    l0.invalidate(&L0Key::new(*ino, kind, None));
                }
                // Remove from path_to_inode so lookup's early dedup
                // return cannot serve stale metadata.
                // Do NOT remove from self.inodes: live FUSE handles
                // (getattr, open, read) reference inodes by number and
                // would get ENOENT. The inode stays alive but will be
                // re-resolved on next lookup via the provider.
                self.path_to_inode.remove(&(mount.to_string(), path.clone()));
                break;
            }
        }
    }

    // Second pass: for each invalidated path, evict the *parent's*
    // lookup(aux=child_name) entry in L0. This is the key correctness
    // fix: lookup checks L0Key(parent_ino, Lookup, Some(child_name))
    // before calling the provider. If "a/b" is invalidated, we must
    // evict L0Key(ino_of_a, Lookup, Some("b")).
    for prefix in &prefixes {
        for pto_entry in self.path_to_inode.iter() {
            let (ref pto_mount, ref pto_path) = *pto_entry.key();
            if pto_mount != mount {
                continue;
            }
            // Check if this path is a child directly under the invalidated prefix.
            if let Some(remainder) = pto_path.strip_prefix(prefix) {
                if !remainder.contains('/') && !remainder.is_empty() {
                    // pto_path is a direct child. Its parent's path is prefix
                    // (stripped of trailing slash). Find parent inode.
                    let parent_path = prefix.trim_end_matches('/');
                    if let Some(parent_ino_ref) = self.path_to_inode.get(&(mount.to_string(), parent_path.to_string())) {
                        let parent_ino = *parent_ino_ref;
                        drop(parent_ino_ref);
                        l0.invalidate(&L0Key::new(parent_ino, RecordKind::Lookup, Some(remainder.to_string())));
                    }
                }
            }
        }
    }
}
```

In `lookup`, `opendir`, and `read`, add a call at the start of the provider-delegated section (after extracting `mount` and before any L0/L2 or `path_to_inode` cache checks):

```rust
self.drain_and_evict_l0(&mount);
```

> **Placement in lookup**: The drain call must go **before** the `path_to_inode`
> check (currently line 166), not after it. Otherwise stale inode entries
> survive invalidation and are served from the early return path.

- [ ] **Step 5: Emit invalidations from GitHub events**

In `providers/github/src/browse/events.rs`, update `invalidate_from_event` to emit host cache invalidation effects in addition to clearing the provider-side JSON cache. Since `invalidate_from_event` is called inside `resume_events` which returns a terminal `ProviderResponse::Done`, we cannot emit effects mid-response. Instead, collect the invalidation prefixes and emit them as a batch from `resume_events`.

Update `resume_events`:

```rust
pub fn resume_events(repos: &[String], effect_outcome: &EffectResult) -> ProviderResponse {
    let results = match effect_outcome {
        EffectResult::Batch(results) => results,
        EffectResult::Single(result) => std::slice::from_ref(result),
    };

    let mut invalidation_prefixes: Vec<String> = Vec::new();

    for (repo, result) in repos.iter().zip(results.iter()) {
        let SingleEffectResult::HttpResponse(resp) = result else {
            continue;
        };
        if resp.status == 401 {
            enter_cache_only();
            continue;
        }
        if resp.status == 304 || resp.status >= 400 {
            continue;
        }
        if let Some(etag) = header_value(&resp.headers, "etag") {
            let _ = with_state(|state| {
                state.event_etags.insert(repo.clone(), etag);
            });
        }
        let Ok(json) = api::parse_json(&resp.body) else {
            continue;
        };
        let Some(events) = json.as_array() else {
            continue;
        };
        for event in events {
            invalidate_from_event(repo, event, &mut invalidation_prefixes);
        }
    }

    // Emit host cache invalidation effects if any prefixes were collected.
    if !invalidation_prefixes.is_empty() {
        invalidation_prefixes.sort();
        invalidation_prefixes.dedup();
        let effects: Vec<SingleEffect> = invalidation_prefixes
            .into_iter()
            .map(|prefix| SingleEffect::CacheInvalidatePrefix(CacheInvalidateRequest { prefix }))
            .collect();
        // Return as a batch; the host will execute each invalidation and
        // resume. Since resume_events is terminal, wrap in a Done after the
        // batch completes. We need a continuation for this.
        // Simpler approach: fire-and-forget via the existing batch mechanism.
        // But resume_events is a terminal handler, so we return Done.
        // The cleanest solution: emit invalidations alongside the existing
        // provider-side cache clearing, using a new continuation.
        //
        // Actually, the simplest correct approach: return the invalidation
        // effects as a Batch, and handle the resume with a no-op continuation.
        // But this requires a new Continuation variant.
        //
        // For minimal change: use the timer_tick return path. timer_tick already
        // returns Done(Ok). We can return Batch(effects) with a new
        // Continuation::InvalidatingCache that returns Done(Ok) on resume.
    }

    ProviderResponse::Done(ActionResult::Ok)
}
```

**Alternative (recommended for minimal change):** Instead of the complex continuation approach above, emit invalidations as individual `SingleEffect` calls during `timer_tick` itself, before the event fetch batch. Since `timer_tick` is the entry point that triggers `resume_events`, refactor to split into two phases:

1. `timer_tick` checks for pending invalidations from the previous tick and emits them.
2. Then proceeds with the event fetch batch as before.

The simplest approach for this phase: keep `invalidate_from_event` clearing the provider-side JSON cache (which already works), and add the host invalidation prefixes to a `Vec<String>` in `ProviderState`. On the next `timer_tick`, emit those as a batch of `CacheInvalidatePrefix` effects before fetching events.

Update `invalidate_from_event` to also record host-facing prefixes:

```rust
pub fn invalidate_from_event(repo: &str, event: &serde_json::Value, host_prefixes: &mut Vec<String>) {
    let Some((owner, name)) = repo.split_once('/') else {
        return;
    };
    let event_type = event
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    match event_type {
        "IssuesEvent" => {
            let prefix = format!("{owner}/{name}/issues/");
            let _ = with_state(|state| state.cache.remove_prefix(&prefix));
            host_prefixes.push(format!("{owner}/{name}/_issues/"));
        }
        "PullRequestEvent" => {
            let prefix = format!("{owner}/{name}/pulls/");
            let _ = with_state(|state| state.cache.remove_prefix(&prefix));
            host_prefixes.push(format!("{owner}/{name}/_prs/"));
        }
        "WorkflowRunEvent" => {
            let prefix = format!("{owner}/{name}/actions/runs/");
            let _ = with_state(|state| state.cache.remove_prefix(&prefix));
            host_prefixes.push(format!("{owner}/{name}/_actions/runs/"));
        }
        "IssueCommentEvent" => {
            if let Some(issue_number) = event
                .get("payload")
                .and_then(|payload| payload.get("issue"))
                .and_then(|issue| issue.get("number"))
                .and_then(serde_json::Value::as_u64)
            {
                let key = format!("{owner}/{name}/issues/{issue_number}/comments");
                let _ = with_state(|state| state.cache.remove(&key));
            }
            // GitHub uses the issues API for both issue and PR comments,
            // so invalidate both browse namespaces.
            host_prefixes.push(format!("{owner}/{name}/_issues/"));
            host_prefixes.push(format!("{owner}/{name}/_prs/"));
        }
        _ => {}
    }
}
```

Then in `timer_tick`, after collecting pending host invalidation prefixes from state, emit them **after** event fetches (not before) so `resume_events` can zip `repos` with results correctly. Track the invalidation count in the continuation.

First, add a new `Continuation` variant in `providers/github/src/lib.rs`:

```rust
FetchingEvents {
    repos: Vec<String>,
    invalidation_count: usize, // number of trailing CacheInvalidatePrefix effects
},
```

Add `pending_host_invalidations: Vec<String>` field to `ProviderState` (initialized to `Vec::new()` in the constructor).

Update `timer_tick`:

```rust
pub fn timer_tick(id: u64) -> ProviderResponse {
    let pending_invalidations = with_state(|state| {
        std::mem::take(&mut state.pending_host_invalidations)
    }).unwrap_or_default();

    let repos = with_state(|state| {
        state.cache.advance_tick();
        state.active_repos.retain(|_, &mut last_touch| {
            let tick = state.cache.current_tick();
            tick.saturating_sub(last_touch) < super::ACTIVE_REPO_TTL
        });
        if state.cache_only { Vec::new() } else {
            state.active_repos.keys().cloned().collect::<Vec<_>>()
        }
    }).unwrap_or_default();

    // Event fetches first (one per repo), invalidations appended after.
    // This keeps the 1:1 alignment between repos[i] and results[i].
    let mut effects: Vec<SingleEffect> = repos
        .iter()
        .filter_map(|repo| {
            let (owner, name) = repo.split_once('/')?;
            Some(events_fetch(owner, name, event_etag(repo)))
        })
        .collect();

    let invalidation_count = pending_invalidations.len();
    effects.extend(pending_invalidations.into_iter().map(|prefix| {
        SingleEffect::CacheInvalidatePrefix(CacheInvalidateRequest { prefix })
    }));

    if effects.is_empty() {
        return ProviderResponse::Done(ActionResult::Ok);
    }

    match with_state(|state| {
        state.pending.insert(
            id,
            Continuation::FetchingEvents { repos, invalidation_count },
        );
    }) {
        Ok(()) => ProviderResponse::Batch(effects),
        Err(e) => err(&e),
    }
}
```

Update `resume_events` to strip trailing invalidation results and store collected prefixes:

```rust
pub fn resume_events(repos: &[String], invalidation_count: usize, effect_outcome: &EffectResult) -> ProviderResponse {
    let all_results = match effect_outcome {
        EffectResult::Batch(results) => results,
        EffectResult::Single(result) => std::slice::from_ref(result),
    };

    // The first repos.len() results correspond to event fetches;
    // the trailing invalidation_count results are CacheOk acks (ignore).
    if all_results.len() != repos.len() + invalidation_count {
        crate::omnifs::provider::log::log(&LogEntry {
            level: LogLevel::Warn,
            message: format!(
                "resume_events: result count mismatch: {} results, {} repos, {} invalidations",
                all_results.len(), repos.len(), invalidation_count
            ),
        });
    }
    let event_results = &all_results[..all_results.len().saturating_sub(invalidation_count)];

    let mut invalidation_prefixes: Vec<String> = Vec::new();

    for (repo, result) in repos.iter().zip(event_results.iter()) {
        let SingleEffectResult::HttpResponse(resp) = result else {
            continue;
        };
        if resp.status == 401 {
            enter_cache_only();
            continue;
        }
        if resp.status == 304 || resp.status >= 400 {
            continue;
        }
        if let Some(etag) = header_value(&resp.headers, "etag") {
            let _ = with_state(|state| {
                state.event_etags.insert(repo.clone(), etag);
            });
        }
        let Ok(json) = api::parse_json(&resp.body) else {
            continue;
        };
        let Some(events) = json.as_array() else {
            continue;
        };
        for event in events {
            invalidate_from_event(repo, event, &mut invalidation_prefixes);
        }
    }

    // Queue collected invalidation prefixes for the next timer tick.
    if !invalidation_prefixes.is_empty() {
        invalidation_prefixes.sort();
        invalidation_prefixes.dedup();
        let _ = with_state(|state| {
            state.pending_host_invalidations.extend(invalidation_prefixes);
        });
    }

    ProviderResponse::Done(ActionResult::Ok)
}
```

Update the `resume` dispatcher in `browse/mod.rs` to pass the new field:

```rust
Continuation::FetchingEvents { repos, invalidation_count } => {
    events::resume_events(&repos, invalidation_count, &effect_outcome)
}
```

- [ ] **Step 6: Build and verify**

Run: `cargo test -p omnifs-host`
Expected: Host cache tests and runtime tests still pass.

Run: `just build-providers`
Expected: Provider components build cleanly with the new invalidation effect.

- [ ] **Step 7: Commit**

```bash
git add crates/host/src/cache/l2.rs crates/host/src/runtime/mod.rs crates/host/src/fuse/mod.rs providers/github/src/browse/events.rs
git commit -m "feat(cache): add provider-driven prefix invalidation

Providers can now invalidate host browse cache prefixes through the
cache-invalidate-prefix effect (added to WIT in Task 1). EffectRuntime
deletes matching L2 records and queues prefixes for FuseFs to drain.
FuseFs evicts matching L0 entries before cache lookups. GitHub event
polling collects invalidation prefixes and emits them on the next
timer tick."
```

**Note:** Do not remove `state.cache.set(cache_key, item_bytes)` from `finalize_search_results` in this phase. Cache-only and stale-on-error browse paths still depend on those raw JSON entries. Generic provider-to-host cache write-back for comments, diffs, and logs is follow-up work.

---

### Task 12: Metrics

**Files:**
- Modify: `crates/host/src/fuse/mod.rs` (lookup, opendir, read)

- [ ] **Step 1: Add tracing-based cache instrumentation**

In `crates/host/src/fuse/mod.rs`, add counter-style tracing events at each cache decision point. Use the existing `tracing` infrastructure with a dedicated target.

At each L0 hit (in lookup, opendir, read), add:

```rust
tracing::debug!(target: "omnifs_cache", kind = "l0_hit", op = "lookup", mount = mount.as_str(), "cache hit");
```

At each L2 hit:

```rust
tracing::debug!(target: "omnifs_cache", kind = "l2_hit", op = "lookup", mount = mount.as_str(), "cache hit");
```

At each provider fallthrough:

```rust
tracing::debug!(target: "omnifs_cache", kind = "miss", op = "lookup", mount = mount.as_str(), "cache miss");
```

At each negative hit:

```rust
tracing::debug!(target: "omnifs_cache", kind = "negative_hit", op = "lookup", mount = mount.as_str(), "negative cache hit");
```

In `extract_projected_files` in runtime:

```rust
tracing::debug!(
    target: "omnifs_cache",
    kind = "prematerialize",
    count = batch.len(),
    "projected files extracted to L2",
);
```

- [ ] **Step 2: Build and verify**

Run: `cargo build --workspace`
Expected: Clean build.

Run: `cargo test -p omnifs-host`
Expected: All tests pass.

- [ ] **Step 3: Run full check**

Run: `just check`
Expected: No format issues, no clippy warnings, provider checks pass, and host tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/host/src/fuse/mod.rs crates/host/src/runtime/mod.rs
git commit -m "feat(cache): add cache hit/miss instrumentation

Tracing events under omnifs_cache target for L0 hits, L2 hits,
cache misses, negative hits, and projected-file extraction counts.
Sits alongside existing FUSE syscall tracing."
```

---

## Verification

After all tasks are complete, the following should hold:

1. `just check` passes cleanly
2. A mounted omnifs with the GitHub provider creates `browse.redb` under `${cache_dir}/providers/${mount}/`
3. After listing `_issues/_open`, subsequent `cat */title */body` reads hit L2/L0 instead of GitHub
4. `RUST_LOG=omnifs_cache=debug` shows L0/L2 hits during `rg` workloads
5. Negative lookups (e.g., `rg` probing nonexistent names) hit the dirents-implied negative path
6. GitHub event polling invalidates matching host cache prefixes within one poll interval, so browse data does not stay stale until TTL expiry
