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
