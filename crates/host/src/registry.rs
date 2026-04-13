//! Provider registry: loading and lifecycle management for WASM providers.
//!
//! Scans the providers directory, instantiates providers via `EffectRuntime`,
//! and manages timer-driven refresh tasks.

use crate::config::InstanceConfig;
use crate::runtime::EffectRuntime;
use crate::runtime::cloner::GitCloner;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// Registry of loaded WASM providers.
///
/// Scans configuration directories, instantiates providers, and manages
/// their lifecycle including timer-driven refresh tasks.
pub struct ProviderRegistry {
    #[allow(dead_code)] // stored for future use (hot-reloading providers)
    engine: wasmtime::Engine,
    instances: HashMap<String, Arc<EffectRuntime>>,
    root_mount: Option<String>,
    timer_shutdown: watch::Sender<bool>,
    timer_tasks: parking_lot::Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl ProviderRegistry {
    pub fn load(
        config_dir: &Path,
        plugin_dir: &Path,
        cloner: &Arc<GitCloner>,
        cache_dir: &Path,
    ) -> Result<Self, RegistryError> {
        let mut wasm_config = wasmtime::Config::new();
        wasm_config.wasm_component_model(true);
        let engine = wasmtime::Engine::new(&wasm_config)
            .map_err(|e| RegistryError::RuntimeError(e.to_string()))?;

        let mut instances = HashMap::new();
        let mut root_mount = None;

        let providers_dir = config_dir.join("providers");
        if !providers_dir.exists() {
            let (timer_shutdown, _) = watch::channel(false);
            return Ok(Self {
                engine,
                instances,
                root_mount,
                timer_shutdown,
                timer_tasks: parking_lot::Mutex::new(Vec::new()),
            });
        }

        for entry in std::fs::read_dir(&providers_dir).map_err(RegistryError::ScanFailed)? {
            let entry = entry.map_err(RegistryError::ScanFailed)?;
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "toml") {
                continue;
            }

            match Self::load_instance(&engine, &path, plugin_dir, cloner, cache_dir) {
                Ok((mount, is_root, runtime)) => {
                    if instances.contains_key(&mount) {
                        tracing::warn!(
                            mount = mount,
                            file = %path.display(),
                            "duplicate mount name; skipping provider (already loaded from another file)"
                        );
                        continue;
                    }
                    if is_root {
                        if let Some(existing) = &root_mount {
                            tracing::warn!(
                                mount = mount,
                                existing = existing.as_str(),
                                "multiple root_mount providers; ignoring root_mount for this one"
                            );
                        } else {
                            root_mount = Some(mount.clone());
                        }
                    }
                    tracing::info!(mount = mount, file = %path.display(), root = is_root, "loaded provider");
                    instances.insert(mount, Arc::new(runtime));
                }
                Err(e) => {
                    tracing::warn!(file = %path.display(), error = %e, "skipping provider");
                }
            }
        }

        let (timer_shutdown, _) = watch::channel(false);
        Ok(Self {
            engine,
            instances,
            root_mount,
            timer_shutdown,
            timer_tasks: parking_lot::Mutex::new(Vec::new()),
        })
    }

    fn load_instance(
        engine: &wasmtime::Engine,
        config_path: &Path,
        plugin_dir: &Path,
        cloner: &Arc<GitCloner>,
        cache_dir: &Path,
    ) -> Result<(String, bool, EffectRuntime), RegistryError> {
        let config = InstanceConfig::from_file(config_path)
            .map_err(|e| RegistryError::ConfigError(e.to_string()))?;

        let wasm_path = plugin_dir.join(&config.plugin);
        if !wasm_path.exists() {
            return Err(RegistryError::PluginNotFound(
                wasm_path.display().to_string(),
            ));
        }

        let is_root = config.root_mount;
        let runtime = EffectRuntime::new(
            engine,
            &wasm_path,
            &config,
            cloner.clone(),
            cache_dir,
            &config.mount,
        )
        .map_err(|e| RegistryError::RuntimeError(e.to_string()))?;

        Ok((config.mount.clone(), is_root, runtime))
    }

    pub fn get(&self, mount: &str) -> Option<&Arc<EffectRuntime>> {
        self.instances.get(mount)
    }

    pub fn mounts(&self) -> Vec<String> {
        self.instances.keys().cloned().collect()
    }

    /// Returns the mount name of the root-mounted provider, if any.
    pub fn root_mount_name(&self) -> Option<&str> {
        self.root_mount.as_deref()
    }

    pub fn shutdown_all(&self) {
        let _ = self.timer_shutdown.send(true);
        for task in self.timer_tasks.lock().drain(..) {
            task.abort();
        }
        for (mount, runtime) in &self.instances {
            if let Err(e) = runtime.shutdown() {
                tracing::warn!(mount, error = %e, "shutdown failed");
            }
        }
    }

    pub fn start_timers(&self, handle: &tokio::runtime::Handle) {
        let mut tasks = self.timer_tasks.lock();
        if !tasks.is_empty() {
            return;
        }

        for (mount, runtime) in &self.instances {
            let interval_secs = match runtime.capabilities() {
                Ok(caps) => caps.refresh_interval_secs,
                Err(e) => {
                    tracing::warn!(mount, error = %e, "failed to read provider capabilities");
                    continue;
                }
            };
            if interval_secs == 0 {
                continue;
            }

            let mount = mount.clone();
            let runtime = runtime.clone();
            let mut shutdown = self.timer_shutdown.subscribe();
            tasks.push(handle.spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(u64::from(interval_secs)));
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            if let Err(e) = runtime.call_timer_tick().await {
                                tracing::debug!(mount = mount.as_str(), error = %e, "provider timer tick failed");
                            }
                        }
                        changed = shutdown.changed() => {
                            if changed.is_ok() {
                                break;
                            }
                        }
                    }
                }
            }));
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("failed to scan providers directory: {0}")]
    ScanFailed(std::io::Error),
    #[error("config error: {0}")]
    ConfigError(String),
    #[error("plugin not found: {0}")]
    PluginNotFound(String),
    #[error("runtime error: {0}")]
    RuntimeError(String),
}
