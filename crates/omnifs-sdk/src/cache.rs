//! LRU cache for provider API payloads.

use std::num::NonZeroUsize;

use lru::LruCache;

pub struct Cache {
    entries: LruCache<String, Vec<u8>>,
    tick: u64,
}

impl Cache {
    pub fn new(max_entries: usize) -> Self {
        let max_entries = NonZeroUsize::new(max_entries.max(1)).unwrap();
        Self {
            entries: LruCache::new(max_entries),
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
        self.entries.get(key).map(|entry| entry.as_slice())
    }

    pub fn set(&mut self, key: String, data: Vec<u8>) {
        self.entries.put(key, data);
    }

    pub fn remove(&mut self, key: &str) {
        self.entries.pop(key);
    }

    pub fn remove_prefix(&mut self, prefix: &str) {
        let keys: Vec<String> = self
            .entries
            .iter()
            .map(|(key, _)| key.as_str())
            .filter(|key| key.starts_with(prefix))
            .map(std::borrow::ToOwned::to_owned)
            .collect();
        for key in keys {
            self.entries.pop(&key);
        }
    }

    pub fn keys_with_prefix(&mut self, prefix: &str) -> Vec<String> {
        self.entries
            .iter()
            .map(|(key, _)| key)
            .filter(|key| key.starts_with(prefix))
            .cloned()
            .collect()
    }
}
