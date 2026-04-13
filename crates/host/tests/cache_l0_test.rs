use omnifs_host::cache::l0::{BrowseCacheL0, L0Key};
use omnifs_host::cache::{CacheRecord, L0_SKIP_THRESHOLD, RecordKind, ttl};

#[test]
fn l0_put_get() {
    let l0 = BrowseCacheL0::new();
    let key = L0Key::new(100, RecordKind::Attr, None);
    let record = CacheRecord::new(
        RecordKind::Attr,
        ttl::ATTR,
        vec![1, 0, 0, 0, 0, 0, 0, 0, 42],
    );
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
    let record = CacheRecord::new(
        RecordKind::Lookup,
        ttl::LOOKUP_POSITIVE,
        vec![1, 1, 0, 0, 0, 0, 0, 0, 0, 42],
    );
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
