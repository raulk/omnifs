use omnifs_host::cache::{
    AttrPayload, CacheRecord, DirentRecord, DirentsPayload, EntryKindCache, LookupPayload,
    RecordKind,
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
        LookupPayload::Positive {
            kind: EntryKindCache::File,
            size: 42
        }
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
        exhaustive: true,
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
