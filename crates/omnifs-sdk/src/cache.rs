//! Tick-based LRU cache for provider data.
//!
//! Entries are evicted by `inserted_at` tick when capacity is reached.
//! Extracted from the GitHub provider's cache as a generic, reusable type.

use hashbrown::HashMap;

struct CachedEntry {
    data: Vec<u8>,
    inserted_at: u64,
}

pub struct Cache {
    entries: HashMap<String, CachedEntry>,
    tick: u64,
    max_entries: usize,
}

impl Cache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            tick: 0,
            max_entries,
        }
    }

    pub fn advance_tick(&mut self) {
        self.tick += 1;
    }

    pub fn current_tick(&self) -> u64 {
        self.tick
    }

    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.entries.get(key).map(|entry| entry.data.as_slice())
    }

    pub fn set(&mut self, key: String, data: Vec<u8>) {
        self.entries.remove(&key);
        if self.entries.len() >= self.max_entries {
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
