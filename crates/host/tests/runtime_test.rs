use omnifs_host::omnifs::provider::types::{ActionResult, EntryKind};
use omnifs_host::runtime::EffectRuntime;
use omnifs_host::runtime::cloner::GitCloner;
use std::path::PathBuf;
use std::sync::Arc;

fn wasm_path() -> std::path::PathBuf {
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let path = workspace_root.join("target/wasm32-wasip1/release/test_provider.wasm");
    assert!(
        path.exists(),
        "test_provider.wasm not found at {path}. Run `just build-providers` first.",
        path = path.display()
    );
    path
}

fn make_engine() -> wasmtime::Engine {
    let mut wasm_config = wasmtime::Config::new();
    wasm_config.wasm_component_model(true);
    wasmtime::Engine::new(&wasm_config).unwrap()
}

fn make_runtime(engine: &wasmtime::Engine) -> EffectRuntime {
    let config = omnifs_host::config::InstanceConfig::parse(
        r#"
        plugin = "test_provider.wasm"
        mount = "test"

        [capabilities]
        domains = ["httpbin.org"]
    "#,
    )
    .unwrap();

    let cloner = Arc::new(GitCloner::new(PathBuf::from("/tmp/omnifs-test-cache")));
    let cache_dir = PathBuf::from("/tmp/omnifs-test-l2");
    EffectRuntime::new(
        engine,
        &wasm_path(),
        &config,
        cloner,
        &cache_dir,
        "test-mount",
    )
    .unwrap()
}

#[tokio::test]
async fn test_initialize() {
    let engine = make_engine();
    let rt = make_runtime(&engine);
    let result = rt.initialize(b"").unwrap();
    match result {
        ActionResult::ProviderInitialized(info) => {
            assert_eq!(info.name, "test-provider");
            assert_eq!(info.version, "0.1.0");
        }
        other => panic!("expected ProviderInitialized, got {other:?}"),
    }
}

#[tokio::test]
async fn test_list_root() {
    let engine = make_engine();
    let rt = make_runtime(&engine);
    let result = rt.call_list_children("").await.unwrap();
    match result {
        ActionResult::DirEntries(listing) => {
            assert_eq!(listing.entries.len(), 1);
            assert_eq!(listing.entries[0].name, "hello");
            assert!(matches!(listing.entries[0].kind, EntryKind::Directory));
        }
        other => panic!("expected DirEntries, got {other:?}"),
    }
}

#[tokio::test]
async fn test_list_hello_dir() {
    let engine = make_engine();
    let rt = make_runtime(&engine);
    let result = rt.call_list_children("hello").await.unwrap();
    match result {
        ActionResult::DirEntries(listing) => {
            assert_eq!(listing.entries.len(), 2);
            let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
            assert!(names.contains(&"message"));
            assert!(names.contains(&"greeting"));
        }
        other => panic!("expected DirEntries, got {other:?}"),
    }
}

#[tokio::test]
async fn test_read_file() {
    let engine = make_engine();
    let rt = make_runtime(&engine);
    let result = rt.call_read_file("hello/message").await.unwrap();
    match result {
        ActionResult::FileContent(data) => {
            assert_eq!(data, b"Hello, world!");
        }
        other => panic!("expected FileContent, got {other:?}"),
    }
}

#[tokio::test]
async fn test_lookup_child() {
    let engine = make_engine();
    let rt = make_runtime(&engine);
    let result = rt.call_lookup_child("", "hello").await.unwrap();
    match result {
        ActionResult::DirEntryOption(Some(entry)) => {
            assert_eq!(entry.name, "hello");
            assert!(matches!(entry.kind, EntryKind::Directory));
        }
        other => panic!("expected DirEntryOption(Some), got {other:?}"),
    }
}

#[tokio::test]
async fn test_lookup_child_not_found() {
    let engine = make_engine();
    let rt = make_runtime(&engine);
    let result = rt.call_lookup_child("", "nonexistent").await.unwrap();
    match result {
        ActionResult::DirEntryOption(None) => {}
        other => panic!("expected DirEntryOption(None), got {other:?}"),
    }
}

/// Tests the effect/resume loop: `read_file("hello/cached")` triggers
/// KV-set -> resume -> KV-get -> resume -> Done(FileContent).
#[tokio::test]
async fn test_effect_resume_loop() {
    let engine = make_engine();
    let rt = make_runtime(&engine);
    let result = rt.call_read_file("hello/cached").await.unwrap();
    match result {
        ActionResult::FileContent(data) => {
            assert_eq!(data, b"cached-value", "expected KV round-trip value");
        }
        other => panic!("expected FileContent from effect chain, got {other:?}"),
    }
}
