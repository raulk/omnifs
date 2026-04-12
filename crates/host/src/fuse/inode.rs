//! Inode allocation and attribute generation for FUSE.
//!
//! Manages the mapping from virtual paths to inode numbers with
//! deduplication and stale entry updates.

use crate::fuse::FuseFs;
use crate::omnifs::provider::types::EntryKind;
use fuser::{FileAttr, FileType, INodeNo};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::SystemTime;

/// Tracks the mapping from inode to (`mount_point`, path) for a provider.
pub(crate) struct InodeEntry {
    pub(crate) mount: String,
    pub(crate) path: String,
    pub(crate) kind: EntryKind,
    pub(crate) size: u64,
    /// When set, FUSE operations for this inode serve directly from the real
    /// filesystem instead of routing through the Wasm provider.
    pub(crate) real_path: Option<PathBuf>,
}

impl FuseFs {
    pub(crate) fn alloc_ino(&self) -> u64 {
        self.next_ino.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn alloc_fh(&self) -> u64 {
        self.next_fh.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn get_or_alloc_ino(
        &self,
        mount: &str,
        path: &str,
        kind: EntryKind,
        size: u64,
    ) -> u64 {
        self.get_or_alloc_ino_inner(mount, path, kind, size, None)
    }

    pub(crate) fn get_or_alloc_ino_real(
        &self,
        mount: &str,
        path: &str,
        kind: EntryKind,
        size: u64,
        real_path: PathBuf,
    ) -> u64 {
        self.get_or_alloc_ino_inner(mount, path, kind, size, Some(real_path))
    }

    fn get_or_alloc_ino_inner(
        &self,
        mount: &str,
        path: &str,
        kind: EntryKind,
        size: u64,
        real_path: Option<PathBuf>,
    ) -> u64 {
        let key = (mount.to_string(), path.to_string());
        // Use entry API to atomically check-or-insert, avoiding a race where
        // two concurrent lookups for the same (mount, path) allocate different inodes.
        // Use and_modify to update kind/size on existing entries (stale inode fix).
        *self
            .path_to_inode
            .entry(key)
            .and_modify(|existing_ino| {
                if let Some(mut entry) = self.inodes.get_mut(existing_ino) {
                    entry.kind = kind;
                    entry.size = size;
                    if real_path.is_some() {
                        entry.real_path.clone_from(&real_path);
                    }
                }
            })
            .or_insert_with(|| {
                let ino = self.alloc_ino();
                self.inodes.insert(
                    ino,
                    InodeEntry {
                        mount: mount.to_string(),
                        path: path.to_string(),
                        kind,
                        size,
                        real_path,
                    },
                );
                ino
            })
    }

    pub(crate) fn dir_attr(ino: u64) -> FileAttr {
        let now = SystemTime::now();
        let (uid, gid) = current_uid_gid();
        FileAttr {
            ino: INodeNo(ino),
            size: 0,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: FileType::Directory,
            perm: 0o555,
            nlink: 2,
            uid,
            gid,
            rdev: 0,
            blksize: 512,
            flags: 0,
        }
    }

    pub(crate) fn file_attr(ino: u64, size: u64) -> FileAttr {
        let now = SystemTime::now();
        let (uid, gid) = current_uid_gid();
        FileAttr {
            ino: INodeNo(ino),
            size,
            blocks: size.div_ceil(512),
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: FileType::RegularFile,
            perm: 0o444,
            nlink: 1,
            uid,
            gid,
            rdev: 0,
            blksize: 512,
            flags: 0,
        }
    }

    /// Build a `FileAttr` from real filesystem metadata.
    pub(crate) fn attr_from_metadata(ino: u64, meta: &std::fs::Metadata) -> FileAttr {
        let kind = if meta.is_dir() {
            FileType::Directory
        } else if meta.is_symlink() {
            FileType::Symlink
        } else {
            FileType::RegularFile
        };
        let perm = if meta.is_dir() { 0o555 } else { 0o444 };
        let nlink = if meta.is_dir() { 2 } else { 1 };
        let now = SystemTime::now();
        let (uid, gid) = current_uid_gid();

        FileAttr {
            ino: INodeNo(ino),
            size: meta.len(),
            blocks: meta.len().div_ceil(512),
            atime: meta.accessed().unwrap_or(now),
            mtime: meta.modified().unwrap_or(now),
            ctime: meta.modified().unwrap_or(now),
            crtime: meta.created().unwrap_or(now),
            kind,
            perm,
            nlink,
            uid,
            gid,
            rdev: 0,
            blksize: 512,
            flags: 0,
        }
    }
}

#[allow(unsafe_code)]
fn current_uid_gid() -> (u32, u32) {
    // SAFETY: getuid and getgid take no pointers, do not require preconditions,
    // and cannot invalidate Rust memory.
    unsafe { (libc::getuid(), libc::getgid()) }
}
