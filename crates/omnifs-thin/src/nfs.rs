//! NFS runner command for `omnifs-thin`.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use crate::host_control::RunnerPhase;
use crate::lifecycle::{AttachPreparation, AttachedRunner, coordinate_mount, prepare_attach};
use anyhow::Context as _;
use omnifs_vfs::Namespace;
use tracing::info;

pub(crate) fn run(args: crate::RunnerArgs) -> anyhow::Result<()> {
    crate::init_tracing();
    let crate::RunnerArgs {
        filesystem,
        spec,
        runtime_instance,
        state_dir,
        attach,
        port,
        host_control,
    } = args;
    let state_dir = state_dir.context("--state-dir is required with --protocol nfs")?;
    let AttachedRunner {
        runtime,
        handle,
        mut lifecycle,
        namespace,
        mount_point,
        ready_port,
    } = prepare_attach(AttachPreparation {
        filesystem: &filesystem,
        spec: &spec,
        runtime_instance,
        state_dir: Some(&state_dir),
        attach,
        runner_control: host_control.into_config()?,
        attach_context: "resolve the VFS attach target",
        preflight_context: "check the NFS mount location",
    })?;

    #[cfg(target_os = "linux")]
    if let Some(port) = ready_port {
        omnifs_vfs::spawn_ready_signal(&handle, mount_point.clone(), port);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = ready_port;
    let mut options = omnifs_nfs::NfsMountOptions::loopback(state_dir);
    options.persist_filehandles = true;
    options.bind = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let cancelled = Arc::clone(&lifecycle.cancelled);
    let mount_point_owned = mount_point.clone();
    let namespace_dyn = Arc::clone(&namespace) as Arc<dyn Namespace>;
    let (mount_done_tx, mount_done_rx) = tokio::sync::oneshot::channel();
    lifecycle.phase.send_replace(RunnerPhase::Mounting);
    let mount_thread = std::thread::Builder::new()
        .name("omnifs-nfs-mount".to_owned())
        .spawn(move || {
            let result = omnifs_nfs::mount_blocking_cancellable(
                &mount_point_owned,
                namespace_dyn,
                handle,
                &options,
                &cancelled,
            )
            .context("serve the NFS mount");
            let _ = mount_done_tx.send(result);
        })
        .context("start the NFS mount owner")?;
    let result = runtime.block_on(coordinate_mount(&spec, &mut lifecycle, mount_done_rx));
    mount_thread
        .join()
        .map_err(|_| anyhow::anyhow!("NFS mount owner panicked"))?;
    result?;

    info!(mount = %mount_point.display(), "filesystem exited");
    Ok(())
}
