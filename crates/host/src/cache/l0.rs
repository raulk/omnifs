//! L0 browse cache: in-memory, inode-keyed, byte-weighted moka cache.

use crate::cache::{CacheRecord, L0_MAX_WEIGHT, L0_SKIP_THRESHOLD, RecordKind};
use moka::sync::Cache;
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct L0Key {
    pub inode: u64,
    pub kind: RecordKind,
    pub aux: Option<String>,
}

impl L0Key {
    pub const fn new(inode: u64, kind: RecordKind, aux: Option<String>) -> Self {
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
                let key_size = 8 + 1 + key.aux.as_ref().map_or(0, String::len);
                let val_size = 2 + value.payload.len();
                (key_size + val_size).try_into().unwrap_or(u32::MAX)
            })
            .build();
        Self { cache }
    }

    pub fn get(&self, key: &L0Key) -> Option<Arc<CacheRecord>> {
        self.cache.get(key)
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
