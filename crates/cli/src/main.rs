//! omnifs-cli: Command-line interface for omnifs.
//!
//! Provides commands to mount and unmount the virtual filesystem,
//! as well as plugin introspection utilities.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "omnifs", about = "omnifs: a virtual filesystem for everything")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Mount {
        #[arg(long)]
        mount_point: String,
        #[arg(long)]
        config_dir: Option<String>,
        #[arg(long)]
        cache_dir: Option<String>,
    },
    Unmount {
        #[arg(long)]
        mount_point: String,
    },
    PluginInfo {
        /// Path to a .wasm provider component
        path: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    #[cfg(target_os = "linux")]
    return run(cli);
    #[cfg(not(target_os = "linux"))]
    return run(&cli);
}

#[cfg(target_os = "linux")]
fn run(cli: Cli) -> anyhow::Result<()> {
    use omnifs_host::registry::ProviderRegistry;
    use omnifs_host::runtime::cloner::GitCloner;
    use std::path::PathBuf;
    use std::sync::Arc;

    match cli.command {
        Commands::Mount {
            mount_point,
            config_dir,
            cache_dir,
        } => {
            let home = dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("/root"));
            let config_path = config_dir.map_or_else(|| home.join(".omnifs"), PathBuf::from);
            let cache_path = cache_dir.map_or_else(|| config_path.join("cache"), PathBuf::from);
            let plugin_dir = config_path.join("plugins");
            let mount_path = PathBuf::from(&mount_point);

            std::fs::create_dir_all(&mount_path)?;
            std::fs::create_dir_all(&cache_path)?;

            // SAFETY: this runs at process startup before provider runtime tasks are spawned.
            #[allow(unsafe_code)]
            unsafe {
                std::env::set_var("OMNIFS_CACHE_DIR", &cache_path);
            }

            // Construct the shared GitCloner early and pass it to all components.
            let cloner = Arc::new(GitCloner::new(cache_path));

            tracing::info!(
                mount_point,
                config = %config_path.display(),
                cache = %cloner.cache_dir().display(),
                "loading providers"
            );

            let registry =
                ProviderRegistry::load(&config_path, &plugin_dir, &cloner, cloner.cache_dir())?;

            for mount_name in registry.mounts() {
                if let Some(runtime) = registry.get(&mount_name) {
                    match runtime.initialize() {
                        Ok(_) => tracing::info!(mount = mount_name, "provider initialized"),
                        Err(e) => tracing::warn!(mount = mount_name, error = %e, "init failed"),
                    }
                }
            }

            let registry = Arc::new(registry);
            let rt = tokio::runtime::Handle::current();
            registry.start_timers(&rt);

            tracing::info!(mount_point, "starting FUSE mount");
            omnifs_host::mount::mount_blocking(&mount_path, &registry, rt)?;
            Ok(())
        }
        Commands::Unmount { mount_point } => {
            omnifs_host::mount::unmount(&PathBuf::from(mount_point))?;
            Ok(())
        }
        Commands::PluginInfo { path: _ } => Err(anyhow::anyhow!("plugin info not yet implemented")),
    }
}

#[cfg(not(target_os = "linux"))]
fn run(cli: &Cli) -> anyhow::Result<()> {
    match &cli.command {
        Commands::Mount { .. } | Commands::Unmount { .. } => Err(anyhow::anyhow!(
            "FUSE mount/unmount is only supported on Linux"
        )),
        Commands::PluginInfo { path: _ } => Err(anyhow::anyhow!("plugin info not yet implemented")),
    }
}
