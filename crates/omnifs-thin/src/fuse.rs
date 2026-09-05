//! FUSE runner command for `omnifs-thin`.

use std::sync::Arc;

use crate::host_control::RunnerPhase;
use crate::lifecycle::{AttachPreparation, AttachedRunner, coordinate_mount, prepare_attach};
use anyhow::Context as _;
use omnifs_mtab::StateFile;
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
    anyhow::ensure!(port == 0, "--port is valid only with --protocol nfs");
    // Parsed (and platform-checked) before the attach dial, so a
    // misconfigured seed fails fast rather than after a 30s connect attempt.
    let AttachedRunner {
        runtime: rt,
        handle,
        mut lifecycle,
        namespace,
        mount_point,
        ready_port,
    } = prepare_attach(AttachPreparation {
        filesystem: &filesystem,
        spec: &spec,
        runtime_instance,
        state_dir: state_dir.as_deref(),
        attach,
        runner_control: host_control.into_config()?,
        attach_context: "resolve the namespace attach target",
        preflight_context: "check the FUSE mount location",
    })?;

    if let Some(port) = ready_port {
        omnifs_vfs::spawn_ready_signal(&handle, mount_point.clone(), port);
    }

    let namespace_dyn = Arc::clone(&namespace) as Arc<dyn Namespace>;
    let notifier = omnifs_fuse::new_notifier_handle();
    let cancelled = Arc::clone(&lifecycle.cancelled);
    let mount_point_owned = mount_point.clone();
    let mount_handle = handle.clone();
    let (mount_done_tx, mount_done_rx) = tokio::sync::oneshot::channel();
    lifecycle.phase.send_replace(RunnerPhase::Mounting);
    let mount_thread = std::thread::Builder::new()
        .name("omnifs-fuse-mount".to_owned())
        .spawn(move || {
            let result = (|| -> anyhow::Result<()> {
                let _state_file = state_dir
                    .as_deref()
                    .map(|dir| StateFile::write_fuse(&mount_point_owned, dir))
                    .transpose()
                    .context("write FUSE mount discovery state")?;
                omnifs_fuse::mount::run_blocking_cancellable(
                    &mount_point_owned,
                    namespace_dyn,
                    &mount_handle,
                    &notifier,
                    &cancelled,
                )
                .context("serve the FUSE mount")
            })();
            let _ = mount_done_tx.send(result);
        })
        .context("start the FUSE mount owner")?;
    let result = rt.block_on(coordinate_mount(&spec, &mut lifecycle, mount_done_rx));
    mount_thread
        .join()
        .map_err(|_| anyhow::anyhow!("FUSE mount owner panicked"))?;
    result?;

    info!(mount = %mount_point.display(), "filesystem exited");
    Ok(())
}
