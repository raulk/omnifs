//! L2 browse cache: durable, path-keyed, per-provider-instance redb database.

use crate::cache::{CacheRecord, L2_BULK_THRESHOLD, RecordKind};
use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;

const METADATA_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata");
const CONTENT_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("content");
const BULK_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("bulk");

type L2Result<T> = anyhow::Result<T>;

pub struct BrowseCacheL2 {
    db: Database,
}

impl BrowseCacheL2 {
    pub fn open(path: &Path) -> L2Result<Self> {
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

    pub fn get(&self, path: &str, kind: RecordKind) -> L2Result<Option<CacheRecord>> {
        let txn = self.db.begin_read()?;
        let key = make_key(path, kind);

        // For File records, check content first, then bulk.
        if kind == RecordKind::File {
            if let Some(record) = Self::read_from_table(&txn, CONTENT_TABLE, &key)? {
                return Ok(Some(record));
            }
            return Self::read_from_table(&txn, BULK_TABLE, &key);
        }

        Self::read_from_table(&txn, METADATA_TABLE, &key)
    }

    pub fn put(&self, path: &str, kind: RecordKind, record: &CacheRecord) -> L2Result<()> {
        let txn = self.db.begin_write()?;
        let key = make_key(path, kind);
        let bytes = record.serialize();
        let target = Self::table_for(kind, record.payload.len());
        {
            let mut table = txn.open_table(target)?;
            table.insert(key.as_str(), bytes.as_slice())?;
        }
        // Remove stale copy from the other file table if the record
        // crossed the bulk threshold since last write.
        if kind == RecordKind::File {
            let is_bulk = record.payload.len() >= L2_BULK_THRESHOLD;
            let other = if is_bulk { CONTENT_TABLE } else { BULK_TABLE };
            let mut other_table = txn.open_table(other)?;
            other_table.remove(key.as_str())?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn put_batch(&self, records: &[(String, RecordKind, CacheRecord)]) -> L2Result<()> {
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
                    _ => {
                        meta.insert(key.as_str(), bytes.as_slice())?;
                    }
                }
            }
        }
        txn.commit()?;
        Ok(())
    }

    fn read_from_table(
        txn: &redb::ReadTransaction,
        table_def: TableDefinition<&str, &[u8]>,
        key: &str,
    ) -> L2Result<Option<CacheRecord>> {
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

impl BrowseCacheL2 {
    /// Delete all records whose logical path starts with `prefix`.
    ///
    /// The stored key format is `{kind_char}:{path}`, so we scan each table
    /// for keys matching `L:{prefix}`, `A:{prefix}`, `D:{prefix}`, `F:{prefix}`
    /// using redb's ordered range iteration.
    pub fn delete_prefix(&self, prefix: &str) -> L2Result<usize> {
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
                    end.push('\u{ffff}');
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
