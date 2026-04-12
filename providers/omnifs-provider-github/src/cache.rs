//! LRU cache with tick-based eviction for API responses.
//!
//! Simple time-based LRU where `inserted_at` ticks determine eviction order.
//! Maximum capacity is hardcoded at 128 entries.

use hashbrown::HashMap;

const MAX_ENTRIES: usize = 128;

struct CachedEntry {
    data: Vec<u8>,
    inserted_at: u64,
}

pub struct Cache {
    entries: HashMap<String, CachedEntry>,
    tick: u64,
}

impl Cache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            tick: 0,
        }
    }

    pub fn advance_tick(&mut self) {
        self.tick += 1;
    }

    pub fn current_tick(&self) -> u64 {
        self.tick
    }

    pub fn get(&mut self, key: &str) -> Option<&[u8]> {
        self.entries.get(key).map(|entry| entry.data.as_slice())
    }

    pub fn set(&mut self, key: String, data: Vec<u8>) {
        // Remove existing entry first (prevents double-count)
        self.entries.remove(&key);
        // Evict entry with smallest inserted_at if at capacity
        if self.entries.len() >= MAX_ENTRIES {
            let oldest_key = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.inserted_at)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest_key {
                self.entries.remove(&k);
            }
        }
        self.entries.insert(
            key,
            CachedEntry {
                data,
                inserted_at: self.tick,
            },
        );
    }

    pub fn remove(&mut self, key: &str) {
        self.entries.remove(key);
    }

    pub fn remove_prefix(&mut self, prefix: &str) {
        self.entries.retain(|key, _| !key.starts_with(prefix));
    }

    pub fn keys_with_prefix(&self, prefix: &str) -> Vec<String> {
        self.entries
            .keys()
            .filter(|key| key.starts_with(prefix))
            .cloned()
            .collect()
    }
}
