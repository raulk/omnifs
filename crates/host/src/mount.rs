//! FUSE mount and unmount operations.
//!
//! Provides `mount_blocking` to start the FUSE filesystem and
//! `unmount` for clean teardown via fusermount.

use crate::fuse::FuseFs;
use crate::registry::ProviderRegistry;
use std::path::Path;
use std::sync::Arc;
use tokio::runtime::Handle;

/// Mount the FUSE filesystem and block until it exits. Calls
/// `registry.shutdown_all()` on exit regardless of how the mount ends.
pub fn mount_blocking(
    mount_point: &Path,
    registry: Arc<ProviderRegistry>,
    rt: Handle,
) -> Result<(), MountError> {
    let fs = FuseFs::new(rt, registry.clone());
    let config = FuseFs::mount_config();

    tracing::info!(mount = %mount_point.display(), "starting FUSE mount");

    let result =
        fuser::mount2(fs, mount_point, &config).map_err(|e| MountError::FuseFailed(e.to_string()));

    tracing::info!("FUSE mount exited, shutting down providers");
    registry.shutdown_all();

    result
}

pub fn unmount(mount_point: &Path) -> Result<(), MountError> {
    let status = std::process::Command::new("fusermount")
        .args(["-u", &mount_point.display().to_string()])
        .status()
        .map_err(|e| MountError::UnmountFailed(e.to_string()))?;

    if status.success() {
        Ok(())
    } else {
        Err(MountError::UnmountFailed(format!(
            "fusermount exited with {}",
            status
        )))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MountError {
    #[error("FUSE mount failed: {0}")]
    FuseFailed(String),
    #[error("unmount failed: {0}")]
    UnmountFailed(String),
}
