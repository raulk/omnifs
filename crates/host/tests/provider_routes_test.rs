use omnifs_host::config::InstanceConfig;
use omnifs_host::omnifs::provider::types::{ActionResult, EntryKind};
use omnifs_host::runtime::EffectRuntime;
use omnifs_host::runtime::cloner::GitCloner;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;

struct RuntimeHarness {
    _engine: wasmtime::Engine,
    _clone_dir: TempDir,
    _cache_dir: TempDir,
    runtime: EffectRuntime,
}

fn provider_wasm_path(plugin_name: &str) -> PathBuf {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let path = workspace_root
        .join("target")
        .join("wasm32-wasip1")
        .join("release")
        .join(plugin_name);
    assert!(
        path.exists(),
        "{plugin_name} not found at {path}. Run `just build-providers` first.",
        path = path.display()
    );
    path
}

fn make_engine() -> wasmtime::Engine {
    let mut wasm_config = wasmtime::Config::new();
    wasm_config.wasm_component_model(true);
    wasmtime::Engine::new(&wasm_config).unwrap()
}

fn make_runtime(config_json: &str) -> RuntimeHarness {
    let config = InstanceConfig::parse(config_json).unwrap();
    let engine = make_engine();
    let clone_dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let cloner = Arc::new(GitCloner::new(clone_dir.path().to_path_buf()));
    let runtime = EffectRuntime::new(
        &engine,
        &provider_wasm_path(&config.plugin),
        &config,
        cloner,
        cache_dir.path(),
        &config.mount,
    )
    .unwrap();

    RuntimeHarness {
        _engine: engine,
        _clone_dir: clone_dir,
        _cache_dir: cache_dir,
        runtime,
    }
}

#[test]
fn dns_provider_exposes_declared_config_schema() {
    let harness = make_runtime(
        r#"
        {
            "plugin": "omnifs_provider_dns.wasm",
            "mount": "dns",
            "capabilities": {
                "domains": ["cloudflare-dns.com", "dns.google"]
            },
            "config": {
                "default_resolver": "cloudflare",
                "resolvers": {
                    "cloudflare": {
                        "url": "https://cloudflare-dns.com/dns-query",
                        "aliases": ["1.1.1.1"]
                    }
                }
            }
        }
    "#,
    );

    let schema = harness.runtime.config_schema().unwrap().unwrap();
    let schema_json: serde_json::Value = serde_json::from_str(&schema).unwrap();

    assert_eq!(
        schema_json["properties"]["default_resolver"]["default"],
        serde_json::Value::String("cloudflare".to_string())
    );
    assert!(schema_json["properties"]["resolvers"].is_object());
}

#[tokio::test]
async fn dns_provider_routes_static_and_dynamic_paths() {
    let harness = make_runtime(
        r#"
        {
            "plugin": "omnifs_provider_dns.wasm",
            "mount": "dns",
            "capabilities": {
                "domains": ["cloudflare-dns.com", "dns.google"]
            }
        }
    "#,
    );
    harness.runtime.initialize().unwrap();

    let lookup = harness
        .runtime
        .call_lookup_child("", "_resolvers")
        .await
        .unwrap();
    match lookup {
        ActionResult::DirEntryOption(Some(entry)) => {
            assert_eq!(entry.name, "_resolvers");
            assert!(matches!(entry.kind, EntryKind::File));
        }
        other => panic!("expected _resolvers file entry, got {other:?}"),
    }

    let listing = harness
        .runtime
        .call_list_children("example.com")
        .await
        .unwrap();
    match listing {
        ActionResult::DirEntries(listing) => {
            let names: Vec<&str> = listing
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert!(names.contains(&"A"));
            assert!(names.contains(&"_all"));
            assert!(names.contains(&"_raw"));
        }
        other => panic!("expected domain listing, got {other:?}"),
    }
}

#[tokio::test]
async fn github_provider_routes_namespace_and_numeric_paths() {
    let harness = make_runtime(
        r#"
        {
            "plugin": "omnifs_provider_github.wasm",
            "mount": "github",
            "capabilities": {
                "domains": ["api.github.com"]
            }
        }
    "#,
    );
    harness.runtime.initialize().unwrap();

    let repo_listing = harness
        .runtime
        .call_list_children("octocat/Hello-World")
        .await
        .unwrap();
    match repo_listing {
        ActionResult::DirEntries(listing) => {
            let names: Vec<&str> = listing
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert_eq!(names, vec!["_repo", "_issues", "_prs", "_actions"]);
        }
        other => panic!("expected repo namespace listing, got {other:?}"),
    }

    let lookup = harness
        .runtime
        .call_lookup_child("octocat/Hello-World/_actions/runs", "123")
        .await
        .unwrap();
    match lookup {
        ActionResult::DirEntryOption(Some(entry)) => {
            assert_eq!(entry.name, "123");
            assert!(matches!(entry.kind, EntryKind::Directory));
        }
        other => panic!("expected action run directory entry, got {other:?}"),
    }

    let run_listing = harness
        .runtime
        .call_list_children("octocat/Hello-World/_actions/runs/123")
        .await
        .unwrap();
    match run_listing {
        ActionResult::DirEntries(listing) => {
            let names: Vec<&str> = listing
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert_eq!(names, vec!["status", "conclusion", "log"]);
            assert!(
                listing
                    .entries
                    .iter()
                    .all(|entry| matches!(entry.kind, EntryKind::File))
            );
        }
        other => panic!("expected action run file listing, got {other:?}"),
    }
}
