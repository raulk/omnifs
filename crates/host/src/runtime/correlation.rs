//! Async effect correlation ID tracking.
//!
//! Assigns unique IDs to async effects so providers can correlate
//! resume calls with pending operations.

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct CorrelationTracker {
    next_id: AtomicU64,
    pending: DashMap<u64, String>,
}

impl CorrelationTracker {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            pending: DashMap::new(),
        }
    }

    pub fn allocate(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn mark_pending(&self, id: u64, operation: String) {
        self.pending.insert(id, operation);
    }

    pub fn is_pending(&self, id: u64) -> bool {
        self.pending.contains_key(&id)
    }

    pub fn resolve(&self, id: u64) -> Option<String> {
        self.pending.remove(&id).map(|(_, op)| op)
    }

    pub fn cancel(&self, id: u64) -> Option<String> {
        self.pending.remove(&id).map(|(_, op)| op)
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

impl Default for CorrelationTracker {
    fn default() -> Self {
        Self::new()
    }
}
