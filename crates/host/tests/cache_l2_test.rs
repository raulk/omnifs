use omnifs_host::cache::l2::BrowseCacheL2;
use omnifs_host::cache::{CacheRecord, RecordKind, ttl};

#[test]
fn l2_put_get_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("browse.redb");
    let l2 = BrowseCacheL2::open(&db_path).unwrap();

    let record = CacheRecord::new(
        RecordKind::Attr,
        ttl::ATTR,
        vec![1, 0, 0, 0, 0, 0, 0, 0, 42],
    );
    l2.put(
        "owner/repo/_issues/_open/1/title",
        RecordKind::Attr,
        &record,
    )
    .unwrap();

    let got = l2
        .get("owner/repo/_issues/_open/1/title", RecordKind::Attr)
        .unwrap();
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
    assert!(
        l2.get("expired/path", RecordKind::Lookup)
            .unwrap()
            .is_none()
    );
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
        (
            "a/title".to_string(),
            RecordKind::File,
            CacheRecord::new(RecordKind::File, ttl::PROJECTED_FILE, b"hello\n".to_vec()),
        ),
        (
            "a/body".to_string(),
            RecordKind::File,
            CacheRecord::new(RecordKind::File, ttl::PROJECTED_FILE, b"world\n".to_vec()),
        ),
        (
            "a".to_string(),
            RecordKind::Attr,
            CacheRecord::new(RecordKind::Attr, ttl::ATTR, vec![0, 0, 0, 0, 0, 0, 0, 0, 0]),
        ),
    ];
    l2.put_batch(&records).unwrap();

    assert!(l2.get("a/title", RecordKind::File).unwrap().is_some());
    assert!(l2.get("a/body", RecordKind::File).unwrap().is_some());
    assert!(l2.get("a", RecordKind::Attr).unwrap().is_some());
}
