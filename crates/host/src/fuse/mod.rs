//! FUSE filesystem implementation.
//!
//! Bridges the omnifs virtual filesystem to the kernel FUSE subsystem.
//! Routes operations to WASM providers. Supports direct filesystem
//! passthrough when providers set real_path on inodes.

pub(crate) mod inode;

use crate::cache::l0::{BrowseCacheL0, L0Key};
use crate::cache::{CacheRecord, RecordKind};
use crate::omnifs::provider::types::{ActionResult, EntryKind};
use crate::registry::ProviderRegistry;
use crate::runtime::EffectRuntime;
use dashmap::DashMap;
use fuser::{
    Errno, FileHandle as FuseFileHandle, Filesystem, FopenFlags, Generation, INodeNo, LockOwner,
    MountOption, OpenFlags, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry,
    ReplyOpen, Request,
};
use inode::InodeEntry;
use std::ffi::OsStr;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};
use tokio::runtime::Handle;

const TTL: Duration = Duration::from_secs(1);
const ROOT_INO: u64 = 1;

struct FuseTrace {
    op: &'static str,
    detail: String,
    start: Instant,
}

impl FuseTrace {
    fn new(op: &'static str, detail: String) -> Self {
        Self {
            op,
            detail,
            start: Instant::now(),
        }
    }
}

impl Drop for FuseTrace {
    fn drop(&mut self) {
        tracing::info!(
            target: "omnifs_trace",
            kind = "fuse",
            op = self.op,
            detail = self.detail.as_str(),
            elapsed_us = self.start.elapsed().as_micros(),
            "trace_event"
        );
    }
}

pub struct FuseFs {
    rt: Handle,
    registry: Arc<ProviderRegistry>,
    inodes: DashMap<u64, InodeEntry>,
    /// Reverse lookup: (mount, path) -> inode, for dedup.
    path_to_inode: DashMap<(String, String), u64>,
    next_ino: AtomicU64,
    dir_snapshots: DashMap<u64, Vec<(u64, String, EntryKind)>>,
    next_fh: AtomicU64,
    /// Caches file content by file handle; populated on first read, evicted on release.
    file_cache: DashMap<u64, Vec<u8>>,
    /// Per-mount L0 browse caches (inode-keyed, in-memory).
    l0_caches: DashMap<String, BrowseCacheL0>,
}

impl FuseFs {
    pub fn new(rt: Handle, registry: Arc<ProviderRegistry>) -> Self {
        let inodes = DashMap::new();

        let root_entry = InodeEntry {
            mount: registry.root_mount_name().unwrap_or("").to_string(),
            path: String::new(),
            kind: EntryKind::Directory,
            size: 0,
            real_path: None,
        };
        inodes.insert(ROOT_INO, root_entry);

        Self {
            rt,
            registry,
            inodes,
            path_to_inode: DashMap::new(),
            next_ino: AtomicU64::new(2),
            dir_snapshots: DashMap::new(),
            next_fh: AtomicU64::new(1),
            file_cache: DashMap::new(),
            l0_caches: DashMap::new(),
        }
    }

    pub fn mount_config() -> fuser::Config {
        let mut config = fuser::Config::default();
        config.mount_options = vec![MountOption::RO, MountOption::FSName("omnifs".to_string())];
        config
    }

    fn runtime_for_mount(&self, mount: &str) -> Option<Arc<EffectRuntime>> {
        self.registry.get(mount).cloned()
    }

    /// Get or lazily create the L0 cache for a mount.
    fn l0_for_mount(&self, mount: &str) -> dashmap::mapref::one::Ref<'_, String, BrowseCacheL0> {
        self.l0_caches
            .entry(mount.to_string())
            .or_insert_with(BrowseCacheL0::new);
        self.l0_caches.get(mount).unwrap()
    }

    fn l0_get(&self, mount: &str, inode: u64, kind: RecordKind, aux: Option<String>) -> Option<std::sync::Arc<CacheRecord>> {
        let l0 = self.l0_for_mount(mount);
        l0.get(&L0Key::new(inode, kind, aux))
    }

    fn l0_put(&self, mount: &str, inode: u64, kind: RecordKind, aux: Option<String>, record: CacheRecord) {
        let l0 = self.l0_for_mount(mount);
        l0.put(L0Key::new(inode, kind, aux), record);
    }
}

impl Filesystem for FuseFs {
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let Some(name_str) = name.to_str() else {
            reply.error(Errno::EINVAL);
            return;
        };
        let _trace = FuseTrace::new("lookup", format!("parent={} name={}", parent.0, name_str));

        let _span =
            tracing::debug_span!("fuse::lookup", parent = parent.0, name = name_str).entered();

        // Synthetic root (no root_mount): mount points are children.
        if parent.0 == ROOT_INO && self.registry.root_mount_name().is_none() {
            if self.registry.get(name_str).is_some() {
                let ino = self.get_or_alloc_ino(name_str, "", EntryKind::Directory, 0);
                reply.entry(&TTL, &self.dir_attr(ino), Generation(0));
                return;
            }
            reply.error(Errno::ENOENT);
            return;
        }
        // When root_mount is set, ROOT_INO falls through to the normal
        // provider delegation path below (its mount field is non-empty).

        let Some(parent_entry) = self.inodes.get(&parent.0) else {
            reply.error(Errno::ENOENT);
            return;
        };
        let mount = parent_entry.mount.clone();
        let parent_path = parent_entry.path.clone();
        let parent_real = parent_entry.real_path.clone();
        drop(parent_entry);

        let child_path = if parent_path.is_empty() {
            name_str.to_string()
        } else {
            format!("{parent_path}/{name_str}")
        };

        // If the parent has a real_path, resolve the child from the filesystem.
        if let Some(ref parent_rp) = parent_real {
            let child_rp = parent_rp.join(name_str);
            match std::fs::symlink_metadata(&child_rp) {
                Ok(meta) => {
                    let kind = if meta.is_dir() {
                        EntryKind::Directory
                    } else {
                        EntryKind::File
                    };
                    let ino =
                        self.get_or_alloc_ino_real(&mount, &child_path, kind, meta.len(), child_rp);
                    let attr = self.attr_from_metadata(ino, &meta);
                    reply.entry(&TTL, &attr, Generation(0));
                }
                Err(_) => reply.error(Errno::ENOENT),
            }
            return;
        }

        let child_key = (mount.clone(), child_path.clone());
        if let Some(ino_ref) = self.path_to_inode.get(&child_key) {
            let ino = *ino_ref;
            drop(ino_ref);
            if let Some(entry) = self.inodes.get(&ino) {
                let attr = match entry.kind {
                    EntryKind::Directory => self.dir_attr(ino),
                    EntryKind::File => self.file_attr(ino, entry.size),
                };
                reply.entry(&TTL, &attr, Generation(0));
                return;
            }
        }

        let Some(runtime) = self.runtime_for_mount(&mount) else {
            reply.error(Errno::ENOENT);
            return;
        };

        match self
            .rt
            .block_on(runtime.call_resolve_entry(&parent_path, name_str))
        {
            Ok(ActionResult::DisownedTree(tree_ref)) => {
                if let Some(real_root) = runtime.resolve_tree_ref(tree_ref) {
                    // Set real_path on the parent so future lookups use passthrough.
                    if let Some(mut parent_entry) = self.inodes.get_mut(&parent.0) {
                        if parent_entry.real_path.is_none() {
                            parent_entry.real_path = Some(real_root.clone());
                        }
                    }
                    let child_rp = real_root.join(name_str);
                    match std::fs::symlink_metadata(&child_rp) {
                        Ok(meta) => {
                            let kind = if meta.is_dir() {
                                EntryKind::Directory
                            } else {
                                EntryKind::File
                            };
                            let ino = self.get_or_alloc_ino_real(
                                &mount,
                                &child_path,
                                kind,
                                meta.len(),
                                child_rp,
                            );
                            let attr = self.attr_from_metadata(ino, &meta);
                            reply.entry(&TTL, &attr, Generation(0));
                        }
                        Err(_) => reply.error(Errno::ENOENT),
                    }
                } else {
                    reply.error(Errno::EIO);
                }
            }
            Ok(ActionResult::DirEntryOption(Some(entry))) => {
                let size = entry.size.unwrap_or(0);
                let ino = self.get_or_alloc_ino(&mount, &child_path, entry.kind, size);
                let attr = match entry.kind {
                    EntryKind::Directory => self.dir_attr(ino),
                    EntryKind::File => self.file_attr(ino, size),
                };
                reply.entry(&TTL, &attr, Generation(0));
            }
            Ok(ActionResult::DirEntryOption(None)) => {
                reply.error(Errno::ENOENT);
            }
            Ok(_) | Err(_) => {
                reply.error(Errno::EIO);
            }
        }
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FuseFileHandle>, reply: ReplyAttr) {
        let _trace = FuseTrace::new("getattr", format!("ino={}", ino.0));
        if ino.0 == ROOT_INO {
            reply.attr(&TTL, &self.dir_attr(ROOT_INO));
            return;
        }

        let Some(entry) = self.inodes.get(&ino.0) else {
            reply.error(Errno::ENOENT);
            return;
        };

        // Passthrough for inodes with real_path.
        if let Some(ref rp) = entry.real_path {
            match std::fs::symlink_metadata(rp) {
                Ok(meta) => {
                    let attr = self.attr_from_metadata(ino.0, &meta);
                    reply.attr(&TTL, &attr);
                }
                Err(_) => reply.error(Errno::ENOENT),
            }
            return;
        }

        let attr = match entry.kind {
            EntryKind::Directory => self.dir_attr(ino.0),
            EntryKind::File => self.file_attr(ino.0, entry.size),
        };
        reply.attr(&TTL, &attr);
    }

    fn opendir(&self, _req: &Request, ino: INodeNo, _flags: OpenFlags, reply: ReplyOpen) {
        let _trace = FuseTrace::new("opendir", format!("ino={}", ino.0));
        let _span = tracing::debug_span!("fuse::opendir", inode = ino.0).entered();

        let fh = self.alloc_fh();

        // Synthetic root (no root_mount): list mount points.
        if ino.0 == ROOT_INO && self.registry.root_mount_name().is_none() {
            let mounts = self.registry.mounts();
            let mut entries = Vec::new();
            for m in mounts {
                let child_ino = self.get_or_alloc_ino(&m, "", EntryKind::Directory, 0);
                entries.push((child_ino, m, EntryKind::Directory));
            }
            self.dir_snapshots.insert(fh, entries);
            reply.opened(FuseFileHandle(fh), FopenFlags::empty());
            return;
        }
        // When root_mount is set, ROOT_INO falls through to provider listing.

        let Some(inode_entry) = self.inodes.get(&ino.0) else {
            reply.error(Errno::ENOENT);
            return;
        };
        let mount = inode_entry.mount.clone();
        let path = inode_entry.path.clone();
        let real = inode_entry.real_path.clone();
        drop(inode_entry);

        // Passthrough for inodes with real_path.
        if let Some(ref rp) = real {
            match std::fs::read_dir(rp) {
                Ok(read_dir) => {
                    let mut snapshot = Vec::new();
                    for dir_entry in read_dir.flatten() {
                        let fname = dir_entry.file_name();
                        let Some(fname_str) = fname.to_str() else {
                            continue;
                        };
                        let child_rp = dir_entry.path();
                        let Ok(meta) = std::fs::symlink_metadata(&child_rp) else {
                            continue;
                        };
                        let kind = if meta.is_dir() {
                            EntryKind::Directory
                        } else {
                            EntryKind::File
                        };
                        let child_path = if path.is_empty() {
                            fname_str.to_string()
                        } else {
                            format!("{path}/{fname_str}")
                        };
                        let child_ino = self.get_or_alloc_ino_real(
                            &mount,
                            &child_path,
                            kind,
                            meta.len(),
                            child_rp,
                        );
                        snapshot.push((child_ino, fname_str.to_string(), kind));
                    }
                    self.dir_snapshots.insert(fh, snapshot);
                    reply.opened(FuseFileHandle(fh), FopenFlags::empty());
                }
                Err(_) => reply.error(Errno::EIO),
            }
            return;
        }

        let Some(runtime) = self.runtime_for_mount(&mount) else {
            reply.error(Errno::ENOENT);
            return;
        };

        match self.rt.block_on(runtime.call_list_entries(&path)) {
            Ok(ActionResult::DisownedTree(tree_ref)) => {
                if let Some(real_root) = runtime.resolve_tree_ref(tree_ref) {
                    // Set real_path on this inode so future operations use passthrough.
                    if let Some(mut entry) = self.inodes.get_mut(&ino.0) {
                        if entry.real_path.is_none() {
                            entry.real_path = Some(real_root.clone());
                        }
                    }
                    match std::fs::read_dir(&real_root) {
                        Ok(read_dir) => {
                            let mut snapshot = Vec::new();
                            for dir_entry in read_dir.flatten() {
                                let fname = dir_entry.file_name();
                                let Some(fname_str) = fname.to_str() else {
                                    continue;
                                };
                                let child_rp = dir_entry.path();
                                let Ok(meta) = std::fs::symlink_metadata(&child_rp) else {
                                    continue;
                                };
                                let kind = if meta.is_dir() {
                                    EntryKind::Directory
                                } else {
                                    EntryKind::File
                                };
                                let child_path = if path.is_empty() {
                                    fname_str.to_string()
                                } else {
                                    format!("{path}/{fname_str}")
                                };
                                let child_ino = self.get_or_alloc_ino_real(
                                    &mount,
                                    &child_path,
                                    kind,
                                    meta.len(),
                                    child_rp,
                                );
                                snapshot.push((child_ino, fname_str.to_string(), kind));
                            }
                            self.dir_snapshots.insert(fh, snapshot);
                            reply.opened(FuseFileHandle(fh), FopenFlags::empty());
                        }
                        Err(_) => reply.error(Errno::EIO),
                    }
                } else {
                    reply.error(Errno::EIO);
                }
            }
            Ok(ActionResult::DirEntries(dir_entries)) => {
                let mut snapshot = Vec::with_capacity(dir_entries.len());
                for e in dir_entries {
                    let child_path = if path.is_empty() {
                        e.name.clone()
                    } else {
                        format!("{path}/{}", e.name)
                    };
                    let size = e.size.unwrap_or(0);
                    let child_ino = self.get_or_alloc_ino(&mount, &child_path, e.kind, size);
                    snapshot.push((child_ino, e.name, e.kind));
                }
                self.dir_snapshots.insert(fh, snapshot);
                reply.opened(FuseFileHandle(fh), FopenFlags::empty());
            }
            _ => {
                reply.error(Errno::EIO);
            }
        }
    }

    fn readdir(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FuseFileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let _trace = FuseTrace::new("readdir", format!("fh={} offset={}", fh.0, offset));
        let Some(snapshot) = self.dir_snapshots.get(&fh.0) else {
            reply.error(Errno::EBADF);
            return;
        };

        for (index, (ino, name, kind)) in snapshot.iter().enumerate().skip(offset as usize) {
            let ftype = match kind {
                EntryKind::Directory => fuser::FileType::Directory,
                EntryKind::File => fuser::FileType::RegularFile,
            };
            let buffer_full = reply.add(INodeNo(*ino), (index + 1) as u64, ftype, name.as_str());
            if buffer_full {
                break;
            }
        }
        reply.ok();
    }

    fn releasedir(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FuseFileHandle,
        _flags: OpenFlags,
        reply: ReplyEmpty,
    ) {
        let _trace = FuseTrace::new("releasedir", format!("fh={}", fh.0));
        self.dir_snapshots.remove(&fh.0);
        reply.ok();
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        fh: FuseFileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        let _trace = FuseTrace::new(
            "read",
            format!("ino={} fh={} offset={} size={}", ino.0, fh.0, offset, size),
        );
        let _span = tracing::debug_span!("fuse::read", inode = ino.0, offset, size).entered();

        // Serve from cache if this file handle already has data.
        if let Some(cached) = self.file_cache.get(&fh.0) {
            let start = offset as usize;
            let end = (start + size as usize).min(cached.len());
            if start >= cached.len() {
                reply.data(&[]);
            } else {
                reply.data(&cached[start..end]);
            }
            return;
        }

        let Some(inode_entry) = self.inodes.get(&ino.0) else {
            reply.error(Errno::ENOENT);
            return;
        };
        let mount = inode_entry.mount.clone();
        let path = inode_entry.path.clone();
        let real = inode_entry.real_path.clone();
        drop(inode_entry);

        // Passthrough for inodes with real_path.
        if let Some(ref rp) = real {
            match std::fs::read(rp) {
                Ok(data) => {
                    let start = offset as usize;
                    let end = (start + size as usize).min(data.len());
                    if start >= data.len() {
                        self.file_cache.insert(fh.0, data);
                        reply.data(&[]);
                    } else {
                        reply.data(&data[start..end]);
                        self.file_cache.insert(fh.0, data);
                    }
                }
                Err(_) => reply.error(Errno::EIO),
            }
            return;
        }

        let Some(runtime) = self.runtime_for_mount(&mount) else {
            reply.error(Errno::ENOENT);
            return;
        };

        match self.rt.block_on(runtime.call_read_file(&path)) {
            Ok(ActionResult::FileContent(data)) => {
                let start = offset as usize;
                let end = (start + size as usize).min(data.len());
                if start >= data.len() {
                    self.file_cache.insert(fh.0, data);
                    reply.data(&[]);
                } else {
                    reply.data(&data[start..end]);
                    self.file_cache.insert(fh.0, data);
                }
            }
            Ok(ActionResult::Err(msg)) => {
                tracing::warn!(path, error = msg, "provider returned error for read_file");
                reply.error(Errno::EIO);
            }
            Ok(other) => {
                tracing::warn!(path, result = ?other, "read_file returned unexpected result");
                reply.error(Errno::EIO);
            }
            Err(e) => {
                tracing::warn!(path, error = %e, "read_file runtime error");
                reply.error(Errno::EIO);
            }
        }
    }

    fn open(&self, _req: &Request, _ino: INodeNo, _flags: OpenFlags, reply: ReplyOpen) {
        let _trace = FuseTrace::new("open", String::new());
        let fh = self.alloc_fh();
        reply.opened(FuseFileHandle(fh), FopenFlags::empty());
    }

    fn release(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FuseFileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        let _trace = FuseTrace::new("release", format!("fh={}", fh.0));
        self.file_cache.remove(&fh.0);
        reply.ok();
    }

    fn readlink(&self, _req: &Request, ino: INodeNo, reply: ReplyData) {
        let _trace = FuseTrace::new("readlink", format!("ino={}", ino.0));
        let Some(entry) = self.inodes.get(&ino.0) else {
            reply.error(Errno::ENOENT);
            return;
        };
        if let Some(ref rp) = entry.real_path {
            match std::fs::read_link(rp) {
                Ok(target) => reply.data(target.as_os_str().as_encoded_bytes()),
                Err(_) => reply.error(Errno::EIO),
            }
        } else {
            reply.error(Errno::EINVAL);
        }
    }
}
