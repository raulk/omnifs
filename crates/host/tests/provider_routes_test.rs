mod support;

use omnifs_host::omnifs::provider::log::Host as ProviderLogHost;
use omnifs_host::omnifs::provider::types::{
    CalloutResult, EntryKind, ErrorKind, Host as ProviderHost, HttpResponse, ListResult, LogEntry,
    LookupResult, OpResult, ProviderEvent, ProviderReturn,
};
use support::{
    create_test_repo, make_engine, make_initialized_runtime, make_runtime_from_config,
    provider_wasm_path,
};
use wasmtime::component::{Component, HasData, Linker, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

fn seed_github_repo_cache(harness: &support::RuntimeHarness, owner: &str, repo: &str) {
    let cache_path = harness
        .clone_dir
        .path()
        .join("github.com")
        .join(owner)
        .join(repo);
    create_test_repo(&cache_path, "Hello from cache\n");
    std::fs::write(
        cache_path.join(".omnifs-clone-url"),
        format!("git@github.com:{owner}/{repo}.git"),
    )
    .unwrap();
}

struct TestHostState {
    wasi: WasiCtx,
    table: ResourceTable,
}

impl WasiView for TestHostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl ProviderHost for TestHostState {}

impl ProviderLogHost for TestHostState {
    fn log(&mut self, _entry: LogEntry) {}
}

impl HasData for TestHostState {
    type Data<'a> = &'a mut TestHostState;
}

struct GithubProviderSession {
    _engine: Engine,
    store: Store<TestHostState>,
    bindings: omnifs_host::Provider,
}

impl GithubProviderSession {
    fn new() -> Self {
        let engine = make_engine();
        let mut linker = Linker::<TestHostState>::new(&engine);
        wasmtime_wasi::p2::add_to_linker_sync::<TestHostState>(&mut linker).unwrap();
        omnifs_host::Provider::add_to_linker::<TestHostState, TestHostState>(
            &mut linker,
            |state| state,
        )
        .unwrap();

        let component =
            Component::from_file(&engine, provider_wasm_path("omnifs_provider_github.wasm"))
                .unwrap();
        let mut store = Store::new(
            &engine,
            TestHostState {
                wasi: WasiCtxBuilder::new().build(),
                table: ResourceTable::new(),
            },
        );

        let bindings = omnifs_host::Provider::instantiate(&mut store, &component, &linker).unwrap();
        let init = bindings
            .omnifs_provider_lifecycle()
            .call_initialize(&mut store, b"{}")
            .unwrap();
        assert!(
            matches!(
                init,
                ProviderReturn {
                    terminal: Some(OpResult::Init(_)),
                    ..
                }
            ),
            "expected provider initialization, got {init:?}"
        );

        Self {
            _engine: engine,
            store,
            bindings,
        }
    }

    fn read_file(&mut self, id: u64, path: &str) -> ProviderReturn {
        self.bindings
            .omnifs_provider_browse()
            .call_read_file(&mut self.store, id, path)
            .unwrap()
    }

    fn list_children(&mut self, id: u64, path: &str) -> ProviderReturn {
        self.bindings
            .omnifs_provider_browse()
            .call_list_children(&mut self.store, id, path)
            .unwrap()
    }

    fn lookup_child(&mut self, id: u64, parent_path: &str, name: &str) -> ProviderReturn {
        self.bindings
            .omnifs_provider_browse()
            .call_lookup_child(&mut self.store, id, parent_path, name)
            .unwrap()
    }

    #[allow(clippy::needless_pass_by_value)]
    fn resume(&mut self, id: u64, outcomes: Vec<CalloutResult>) -> ProviderReturn {
        self.bindings
            .omnifs_provider_resume()
            .call_resume(&mut self.store, id, &outcomes)
            .unwrap()
    }

    fn timer_tick_with_paths(
        &mut self,
        id: u64,
        active_paths: Vec<omnifs_host::omnifs::provider::types::ActivePathSet>,
    ) -> ProviderReturn {
        self.bindings
            .omnifs_provider_notify()
            .call_on_event(
                &mut self.store,
                id,
                &ProviderEvent::TimerTick(omnifs_host::omnifs::provider::types::TimerTickContext {
                    active_paths,
                }),
            )
            .unwrap()
    }
}

fn invoke_github_read_route(path: &str) -> ProviderReturn {
    let mut session = GithubProviderSession::new();
    session.read_file(1, path)
}

#[test]
fn dns_provider_exposes_declared_config_schema() {
    fn resolve_local_ref<'a>(
        root: &'a serde_json::Value,
        schema: &'a serde_json::Value,
    ) -> &'a serde_json::Value {
        let Some(reference) = schema["$ref"].as_str() else {
            return schema;
        };

        reference
            .trim_start_matches("#/")
            .split('/')
            .fold(root, |current, segment| &current[segment])
    }

    let harness = make_runtime_from_config(
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
    let resolver_value_schema = resolve_local_ref(
        &schema_json,
        &schema_json["properties"]["resolvers"]["additionalProperties"],
    );
    assert_eq!(
        schema_json["properties"]["resolvers"]["type"],
        serde_json::Value::String("object".to_string())
    );
    assert_eq!(
        resolver_value_schema["type"],
        serde_json::Value::String("object".to_string())
    );
    assert_eq!(
        resolver_value_schema["properties"]["url"]["type"],
        serde_json::Value::String("string".to_string())
    );
    assert_eq!(
        resolver_value_schema["properties"]["aliases"]["type"],
        serde_json::Value::String("array".to_string())
    );
    assert_eq!(
        resolver_value_schema["properties"]["aliases"]["items"]["type"],
        serde_json::Value::String("string".to_string())
    );
}

#[test]
fn dns_provider_rejects_invalid_default_resolver_config_during_initialize() {
    let harness = make_runtime_from_config(
        r#"
        {
            "plugin": "omnifs_provider_dns.wasm",
            "mount": "dns",
            "capabilities": {
                "domains": ["cloudflare-dns.com", "dns.google"]
            },
            "config": {
                "default_resolver": "missing",
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

    let result = harness.runtime.initialize().unwrap();
    match result {
        OpResult::Err(error) => {
            assert_eq!(error.kind, ErrorKind::InvalidInput);
            assert!(
                error.message.contains("default resolver"),
                "unexpected error: {error:?}"
            );
        },
        other => panic!("expected initialize-time config error, got {other:?}"),
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn dns_provider_routes_static_and_dynamic_paths() {
    let harness = make_initialized_runtime(
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

    let lookup = harness
        .runtime
        .call_lookup_child("", "_resolvers")
        .await
        .unwrap();
    match lookup {
        OpResult::Lookup(LookupResult::Entry(result)) => {
            let entry = &result.target;
            assert_eq!(entry.name, "_resolvers");
            assert!(matches!(entry.kind, EntryKind::File));
        },
        other => panic!("expected Lookup, got {other:?}"),
    }

    let resolvers_file = harness.runtime.call_read_file("_resolvers").await.unwrap();
    match resolvers_file {
        OpResult::Read(result) => {
            let body = String::from_utf8(result.content).expect("utf8 resolvers file");
            assert!(
                body.contains("cloudflare"),
                "unexpected resolvers file: {body}"
            );
        },
        other => panic!("expected File, got {other:?}"),
    }

    let reverse_lookup = harness
        .runtime
        .call_lookup_child("", "_reverse")
        .await
        .unwrap();
    match reverse_lookup {
        OpResult::Lookup(LookupResult::Entry(result)) => {
            let entry = &result.target;
            assert_eq!(entry.name, "_reverse");
            assert!(matches!(entry.kind, EntryKind::Directory));
        },
        other => panic!("expected Lookup, got {other:?}"),
    }

    let resolver_lookup = harness
        .runtime
        .call_lookup_child("", "@cloudflare")
        .await
        .unwrap();
    match resolver_lookup {
        OpResult::Lookup(LookupResult::Entry(result)) => {
            let entry = &result.target;
            assert_eq!(entry.name, "@cloudflare");
            assert!(matches!(entry.kind, EntryKind::Directory));
        },
        other => panic!("expected Lookup, got {other:?}"),
    }

    let resolver_domain_lookup = harness
        .runtime
        .call_lookup_child("@cloudflare", "example.com")
        .await
        .unwrap();
    match resolver_domain_lookup {
        OpResult::Lookup(LookupResult::Entry(result)) => {
            let entry = &result.target;
            assert_eq!(entry.name, "example.com");
            assert!(matches!(entry.kind, EntryKind::Directory));
        },
        other => panic!("expected Lookup, got {other:?}"),
    }

    let resolver_reverse_lookup = harness
        .runtime
        .call_lookup_child("@cloudflare", "_reverse")
        .await
        .unwrap();
    match resolver_reverse_lookup {
        OpResult::Lookup(LookupResult::Entry(result)) => {
            let entry = &result.target;
            assert_eq!(entry.name, "_reverse");
            assert!(matches!(entry.kind, EntryKind::Directory));
        },
        other => panic!("expected resolver reverse lookup, got {other:?}"),
    }

    let reverse_ip_lookup = harness
        .runtime
        .call_lookup_child("_reverse", "8.8.8.8")
        .await
        .unwrap();
    match reverse_ip_lookup {
        OpResult::Lookup(LookupResult::Entry(result)) => {
            let entry = &result.target;
            assert_eq!(entry.name, "8.8.8.8");
            assert!(matches!(entry.kind, EntryKind::File));
            assert!(result.siblings.is_empty());
        },
        other => panic!("expected reverse IP lookup, got {other:?}"),
    }

    let resolver_reverse_ip_lookup = harness
        .runtime
        .call_lookup_child("@cloudflare/_reverse", "8.8.8.8")
        .await
        .unwrap();
    match resolver_reverse_ip_lookup {
        OpResult::Lookup(LookupResult::Entry(result)) => {
            let entry = &result.target;
            assert_eq!(entry.name, "8.8.8.8");
            assert!(matches!(entry.kind, EntryKind::File));
            assert!(result.siblings.is_empty());
        },
        other => panic!("expected resolver-qualified reverse IP lookup, got {other:?}"),
    }

    let invalid_reverse_lookup = harness
        .runtime
        .call_lookup_child("_reverse", "not-an-ip")
        .await
        .unwrap();
    match invalid_reverse_lookup {
        OpResult::Lookup(LookupResult::NotFound) => {},
        other => panic!("expected invalid reverse lookup NotFound, got {other:?}"),
    }

    let invalid_resolver_reverse_lookup = harness
        .runtime
        .call_lookup_child("@cloudflare/_reverse", "not-an-ip")
        .await
        .unwrap();
    match invalid_resolver_reverse_lookup {
        OpResult::Lookup(LookupResult::NotFound) => {},
        other => panic!("expected invalid resolver reverse lookup NotFound, got {other:?}"),
    }

    let direct_ip_lookup = harness
        .runtime
        .call_lookup_child("", "8.8.8.8")
        .await
        .unwrap();
    match direct_ip_lookup {
        OpResult::Lookup(LookupResult::NotFound) => {},
        other => panic!("expected root direct-IP lookup NotFound, got {other:?}"),
    }

    let resolver_direct_ip_lookup = harness
        .runtime
        .call_lookup_child("@cloudflare", "8.8.8.8")
        .await
        .unwrap();
    match resolver_direct_ip_lookup {
        OpResult::Lookup(LookupResult::NotFound) => {},
        other => panic!("expected resolver direct-IP lookup NotFound, got {other:?}"),
    }

    let domain_lookup = harness
        .runtime
        .call_lookup_child("", "example.com")
        .await
        .unwrap();
    match domain_lookup {
        OpResult::Lookup(LookupResult::Entry(result)) => {
            let entry = &result.target;
            assert_eq!(entry.name, "example.com");
            assert!(matches!(entry.kind, EntryKind::Directory));
            let names: Vec<&str> = result
                .siblings
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert!(names.contains(&"A"));
            assert!(names.contains(&"AAAA"));
            assert!(names.contains(&"_all"));
            assert!(names.contains(&"_raw"));
        },
        other => panic!("expected domain lookup, got {other:?}"),
    }

    let listing = harness
        .runtime
        .call_list_children("example.com")
        .await
        .unwrap();
    match listing {
        OpResult::List(ListResult::Entries(listing)) => {
            let names: Vec<&str> = listing
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert!(names.contains(&"A"));
            assert!(names.contains(&"_all"));
            assert!(names.contains(&"_raw"));
        },
        other => panic!("expected domain listing, got {other:?}"),
    }

    let reverse_listing = harness
        .runtime
        .call_list_children("_reverse")
        .await
        .unwrap();
    match reverse_listing {
        OpResult::List(ListResult::Entries(listing)) => {
            assert!(
                listing.entries.is_empty(),
                "reverse dir should not eagerly list dynamic children: {listing:?}"
            );
        },
        other => panic!("expected reverse dir listing, got {other:?}"),
    }

    let resolver_reverse_listing = harness
        .runtime
        .call_list_children("@cloudflare/_reverse")
        .await
        .unwrap();
    match resolver_reverse_listing {
        OpResult::List(ListResult::Entries(listing)) => {
            assert!(
                listing.entries.is_empty(),
                "resolver reverse dir should not eagerly list dynamic children: {listing:?}"
            );
        },
        other => panic!("expected resolver reverse dir listing, got {other:?}"),
    }
}

#[tokio::test]
async fn dns_provider_activity_tracks_concrete_dispatched_paths() {
    let harness = make_initialized_runtime(
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

    let resolvers_file = harness.runtime.call_read_file("_resolvers").await.unwrap();
    assert!(matches!(resolvers_file, OpResult::Read(_)));

    let resolver_domain_lookup = harness
        .runtime
        .call_lookup_child("@cloudflare", "example.com")
        .await
        .unwrap();
    assert!(matches!(resolver_domain_lookup, OpResult::Lookup(_)));

    let reverse_ip_lookup = harness
        .runtime
        .call_lookup_child("_reverse", "8.8.8.8")
        .await
        .unwrap();
    assert!(matches!(reverse_ip_lookup, OpResult::Lookup(_)));

    let resolver_reverse_ip_lookup = harness
        .runtime
        .call_lookup_child("@cloudflare/_reverse", "8.8.8.8")
        .await
        .unwrap();
    assert!(matches!(resolver_reverse_ip_lookup, OpResult::Lookup(_)));

    let active = harness.runtime.__active_path_sets();

    let root = active
        .iter()
        .find(|entry| entry.mount_id == "/")
        .expect("missing root activity");
    assert_eq!(root.paths, vec!["/"]);

    let resolvers = active
        .iter()
        .find(|entry| entry.mount_id == "/_resolvers")
        .expect("missing resolvers activity");
    assert_eq!(resolvers.paths, vec!["/_resolvers"]);

    let resolver_root = active
        .iter()
        .find(|entry| entry.mount_id == "/@{resolver}")
        .unwrap_or_else(|| panic!("missing resolver-root activity in {active:?}"));
    assert_eq!(resolver_root.paths, vec!["/@cloudflare"]);

    let dns_segment = active
        .iter()
        .find(|entry| entry.mount_id == "/@{resolver}/{domain}")
        .expect("missing dns-segment activity");
    assert_eq!(dns_segment.paths, vec!["/@cloudflare/example.com"]);
    assert!(!dns_segment.paths.iter().any(|path| path == "/_resolvers"));
    assert!(!dns_segment.paths.iter().any(|path| path == "/@cloudflare"));

    let reverse_dir = active
        .iter()
        .find(|entry| entry.mount_id == "/_reverse")
        .expect("missing reverse-dir activity");
    assert_eq!(reverse_dir.paths, vec!["/_reverse"]);

    let resolver_reverse_dir = active
        .iter()
        .find(|entry| entry.mount_id == "/@{resolver}/_reverse")
        .expect("missing resolver-reverse-dir activity");
    assert_eq!(resolver_reverse_dir.paths, vec!["/@cloudflare/_reverse"]);

    let reverse_ip = active
        .iter()
        .find(|entry| entry.mount_id == "/_reverse/{ip}")
        .expect("missing reverse-ip activity");
    assert_eq!(reverse_ip.paths, vec!["/_reverse/8.8.8.8"]);

    let resolver_reverse_ip = active
        .iter()
        .find(|entry| entry.mount_id == "/@{resolver}/_reverse/{ip}")
        .expect("missing resolver-reverse-ip activity");
    assert_eq!(
        resolver_reverse_ip.paths,
        vec!["/@cloudflare/_reverse/8.8.8.8"]
    );

    assert!(
        !dns_segment
            .paths
            .iter()
            .any(|path| path.contains("/_reverse")),
        "dns segment activity should stay domain-only: {active:?}"
    );
}

#[tokio::test]
async fn dns_provider_unknown_resolver_read_is_invalid_input() {
    let harness = make_initialized_runtime(
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

    let result = harness
        .runtime
        .call_read_file("@missing/example.com/A")
        .await
        .unwrap();
    match result {
        OpResult::Err(error) => {
            assert_eq!(error.kind, ErrorKind::InvalidInput);
            assert!(
                error.message.contains("unknown resolver specifier"),
                "unexpected resolver error: {error:?}"
            );
        },
        other => panic!("expected invalid-input resolver error, got {other:?}"),
    }
}

#[tokio::test]
async fn dns_provider_unknown_record_reads_are_not_found() {
    let harness = make_initialized_runtime(
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

    let result = harness
        .runtime
        .call_read_file("example.com/BOGUS")
        .await
        .unwrap();
    match result {
        OpResult::Err(error) => {
            assert_eq!(error.kind, ErrorKind::NotFound);
        },
        other => panic!("expected unknown-record NotFound, got {other:?}"),
    }

    let result = harness
        .runtime
        .call_read_file("@cloudflare/example.com/BOGUS")
        .await
        .unwrap();
    match result {
        OpResult::Err(error) => {
            assert_eq!(error.kind, ErrorKind::NotFound);
        },
        other => panic!("expected resolver unknown-record NotFound, got {other:?}"),
    }
}

#[tokio::test]
async fn github_provider_routes_namespace_and_numeric_paths() {
    let harness = make_initialized_runtime(
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

    let repo_listing = harness
        .runtime
        .call_list_children("octocat/Hello-World")
        .await
        .unwrap();
    match repo_listing {
        OpResult::List(ListResult::Entries(listing)) => {
            let mut names: Vec<&str> = listing
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            names.sort_unstable();
            assert_eq!(names, vec!["_actions", "_issues", "_prs", "_repo"]);
        },
        other => panic!("expected repo namespace listing, got {other:?}"),
    }

    let lookup = harness
        .runtime
        .call_lookup_child("octocat/Hello-World/_actions", "runs")
        .await
        .unwrap();
    match lookup {
        OpResult::Lookup(LookupResult::Entry(result)) => {
            let entry = &result.target;
            assert_eq!(entry.name, "runs");
            assert!(matches!(entry.kind, EntryKind::Directory));
        },
        other => panic!("expected Lookup(runs), got {other:?}"),
    }

    // Note: projected sibling-file lookups (`.../1/title`, `.../1/diff`)
    // are intentionally not asserted here. These files do not have
    // dedicated provider lookup handlers; the host's FuseFs resolves
    // them positively from
    // the parent's cached sibling entries (see d4e9e98's
    // dirents-implied positive path). `CalloutRuntime::call_lookup_child`
    // bypasses that cache and dispatches straight to the provider, so
    // it would return NotFound for them in isolation. Read-path
    // coverage for the same leaves lives in
    // `github_provider_read_routes_dispatch_async_handlers` and
    // `github_provider_resource_reads_do_not_fall_back_to_provider_cache`.
}

#[test]
fn github_issue_list_preloads_projected_files_from_search_results() {
    use omnifs_host::omnifs::provider::types::{Callout, CalloutResult, HttpResponse};

    let mut session = GithubProviderSession::new();
    let response = session.list_children(40, "octocat/Hello-World/_issues/_open");
    assert!(
        response.is_suspended(),
        "expected suspended response, got {response:?}"
    );
    let [Callout::Fetch(fetch)] = response.callouts.as_slice() else {
        panic!("expected fetch effect, got {:?}", response.callouts);
    };
    assert!(
        fetch.url.contains("/search/issues?"),
        "unexpected issue list URL: {}",
        fetch.url
    );

    let response = session.resume(
        40,
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: br#"{
                "items":[
                    {
                        "number":7,
                        "title":"Issue title",
                        "body":"Issue body",
                        "state":"open",
                        "user":{"login":"octocat"}
                    }
                ]
            }"#
            .to_vec(),
        })],
    );

    // Preloads now ride alongside the terminal listing, not as callouts.
    assert!(
        response.callouts.is_empty(),
        "list terminal should carry no callouts, got {:?}",
        response.callouts
    );
    match response.terminal {
        Some(OpResult::List(ListResult::Entries(listing))) => {
            let preload_paths: Vec<&str> = listing
                .preload
                .iter()
                .map(|file| file.path.as_str())
                .collect();
            assert_eq!(
                preload_paths,
                vec![
                    "octocat/Hello-World/_issues/_open/7/title",
                    "octocat/Hello-World/_issues/_open/7/body",
                    "octocat/Hello-World/_issues/_open/7/state",
                    "octocat/Hello-World/_issues/_open/7/user",
                ]
            );
            let names: Vec<&str> = listing
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert_eq!(names, vec!["7"]);
        },
        other => panic!("expected issue listing terminal, got {other:?}"),
    }
}

#[test]
fn github_pr_list_preloads_projected_files_from_search_results() {
    use omnifs_host::omnifs::provider::types::{Callout, CalloutResult, HttpResponse};

    let mut session = GithubProviderSession::new();
    let response = session.list_children(41, "octocat/Hello-World/_prs/_open");
    assert!(
        response.is_suspended(),
        "expected suspended response, got {response:?}"
    );
    let [Callout::Fetch(fetch)] = response.callouts.as_slice() else {
        panic!("expected fetch effect, got {:?}", response.callouts);
    };
    assert!(
        fetch.url.contains("/search/issues?"),
        "unexpected PR list URL: {}",
        fetch.url
    );

    let response = session.resume(
        41,
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: br#"{
                "items":[
                    {
                        "number":7,
                        "title":"PR title",
                        "body":"PR body",
                        "state":"open",
                        "user":{"login":"octocat"}
                    }
                ]
            }"#
            .to_vec(),
        })],
    );

    assert!(
        response.callouts.is_empty(),
        "list terminal should carry no callouts, got {:?}",
        response.callouts
    );
    match response.terminal {
        Some(OpResult::List(ListResult::Entries(listing))) => {
            let preload_paths: Vec<&str> = listing
                .preload
                .iter()
                .map(|file| file.path.as_str())
                .collect();
            assert_eq!(
                preload_paths,
                vec![
                    "octocat/Hello-World/_prs/_open/7/title",
                    "octocat/Hello-World/_prs/_open/7/body",
                    "octocat/Hello-World/_prs/_open/7/state",
                    "octocat/Hello-World/_prs/_open/7/user",
                ]
            );
            let names: Vec<&str> = listing
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert_eq!(names, vec!["7"]);
        },
        other => panic!("expected PR listing terminal, got {other:?}"),
    }
}

#[test]
fn github_action_run_list_preloads_projected_files() {
    use omnifs_host::omnifs::provider::types::{Callout, CalloutResult, HttpResponse};

    let mut session = GithubProviderSession::new();
    let response = session.list_children(42, "octocat/Hello-World/_actions/runs");
    assert!(
        response.is_suspended(),
        "expected suspended response, got {response:?}"
    );
    let [Callout::Fetch(fetch)] = response.callouts.as_slice() else {
        panic!("expected fetch effect, got {:?}", response.callouts);
    };
    assert!(
        fetch
            .url
            .ends_with("/repos/octocat/Hello-World/actions/runs?per_page=30"),
        "unexpected action runs URL: {}",
        fetch.url
    );

    let response = session.resume(
        42,
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: br#"{
                "workflow_runs":[
                    {
                        "id":123,
                        "status":"completed",
                        "conclusion":"success"
                    }
                ]
            }"#
            .to_vec(),
        })],
    );

    assert!(
        response.callouts.is_empty(),
        "list terminal should carry no callouts, got {:?}",
        response.callouts
    );
    match response.terminal {
        Some(OpResult::List(ListResult::Entries(listing))) => {
            let preload_paths: Vec<&str> = listing
                .preload
                .iter()
                .map(|file| file.path.as_str())
                .collect();
            assert_eq!(
                preload_paths,
                vec![
                    "octocat/Hello-World/_actions/runs/123/status",
                    "octocat/Hello-World/_actions/runs/123/conclusion",
                ]
            );
            let names: Vec<&str> = listing
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert_eq!(names, vec!["123"]);
        },
        other => panic!("expected action run listing terminal, got {other:?}"),
    }
}

#[test]
fn github_provider_action_run_lookup_validates_and_listing_validates() {
    use omnifs_host::omnifs::provider::types::{
        Callout, CalloutResult, Header, HttpRequest, HttpResponse,
    };

    #[allow(clippy::needless_pass_by_value)]
    fn expect_fetch(response: ProviderReturn) -> HttpRequest {
        let ProviderReturn {
            terminal: None,
            callouts,
            ..
        } = &response
        else {
            panic!("expected callouts response, got {response:?}");
        };
        let [Callout::Fetch(request)] = callouts.as_slice() else {
            panic!("expected fetch callout, got {response:?}");
        };
        request.clone()
    }

    let mut session = GithubProviderSession::new();

    let lookup_fetch =
        expect_fetch(session.lookup_child(7, "octocat/Hello-World/_actions/runs", "123"));
    assert!(
        lookup_fetch
            .url
            .ends_with("/repos/octocat/Hello-World/actions/runs/123"),
        "unexpected action run lookup URL: {}",
        lookup_fetch.url
    );

    let lookup = session.resume(
        7,
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 200,
            headers: Vec::<Header>::new(),
            body: br#"{"id":123,"status":"completed","conclusion":"success"}"#.to_vec(),
        })],
    );
    match lookup {
        ProviderReturn {
            terminal: Some(OpResult::Lookup(LookupResult::Entry(result))),
            ..
        } => {
            let entry = &result.target;
            assert_eq!(entry.name, "123");
            assert!(matches!(entry.kind, EntryKind::Directory));
            let child_names: Vec<&str> = result
                .siblings
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            let sibling_file_names: Vec<&str> = result
                .sibling_files
                .iter()
                .map(|file| file.name.as_str())
                .collect();
            assert!(
                sibling_file_names.contains(&"status"),
                "missing status in {sibling_file_names:?}"
            );
            assert!(
                sibling_file_names.contains(&"conclusion"),
                "missing conclusion in {sibling_file_names:?}"
            );
            assert!(
                child_names.contains(&"log"),
                "missing log in {child_names:?}"
            );
        },
        other => panic!("expected validated action run lookup result, got {other:?}"),
    }

    let issued = session.list_children(7, "octocat/Hello-World/_actions/runs/123");
    assert!(
        issued.is_suspended(),
        "expected action run listing to dispatch validation, got {issued:?}"
    );

    let listed = session.resume(
        7,
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 200,
            headers: Vec::<Header>::new(),
            body: br#"{"id":123,"status":"completed","conclusion":"success"}"#.to_vec(),
        })],
    );

    match listed {
        ProviderReturn {
            terminal: Some(OpResult::List(ListResult::Entries(result))),
            ..
        } => {
            let names: Vec<&str> = result
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert!(names.contains(&"log"), "missing log in {names:?}");
        },
        other => panic!("expected DirEntries(123) after 200, got {other:?}"),
    }
}

#[tokio::test]
async fn github_owner_listing_tracks_browsed_repos() {
    let harness = make_initialized_runtime(
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

    let repo_listing = harness
        .runtime
        .call_list_children("octocat/Hello-World")
        .await
        .unwrap();
    assert!(
        matches!(repo_listing, OpResult::List(_)),
        "expected repo listing, got {repo_listing:?}"
    );

    let owner_listing = harness.runtime.call_list_children("octocat").await.unwrap();
    match owner_listing {
        OpResult::List(ListResult::Entries(listing)) => {
            let names: Vec<&str> = listing
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert!(
                names.contains(&"Hello-World"),
                "expected Hello-World in owner listing, got {names:?}"
            );
        },
        other => panic!("expected owner listing, got {other:?}"),
    }
}

#[tokio::test]
async fn github_root_and_owner_listings_ignore_unclassified_repo_paths() {
    let harness = make_initialized_runtime(
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

    for path in ["zeta/zulu", "open/source", "alpha/app", "openai/api"] {
        let repo_listing = harness.runtime.call_list_children(path).await.unwrap();
        assert!(
            matches!(repo_listing, OpResult::List(_)),
            "expected repo listing for {path}, got {repo_listing:?}"
        );
    }

    let root_listing = harness.runtime.call_list_children("").await.unwrap();
    match root_listing {
        OpResult::List(ListResult::Entries(listing)) => {
            let names: Vec<&str> = listing
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert!(names.is_empty(), "unexpected root names: {names:?}");
        },
        other => panic!("expected root listing, got {other:?}"),
    }

    let owner_listing = harness.runtime.call_list_children("open").await.unwrap();
    match owner_listing {
        OpResult::List(ListResult::Entries(listing)) => {
            let names: Vec<&str> = listing
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert!(
                names.is_empty(),
                "unexpected owner names after uncached repo traversal: {names:?}"
            );
        },
        other => panic!("expected owner listing, got {other:?}"),
    }
}

#[tokio::test]
async fn github_repo_tree_lists_looks_up_and_reads_from_git_cache() {
    let harness = make_initialized_runtime(
        r#"
        {
            "plugin": "omnifs_provider_github.wasm",
            "mount": "github",
            "capabilities": {
                "domains": ["api.github.com"],
                "git_repos": ["git@github.com:octocat/Hello-World.git"]
            }
        }
    "#,
    );
    seed_github_repo_cache(&harness, "octocat", "Hello-World");

    let repo_listing = harness
        .runtime
        .call_list_children("octocat/Hello-World/_repo")
        .await
        .unwrap();
    match repo_listing {
        OpResult::List(ListResult::Subtree(tree_ref)) => {
            let real_root = harness
                .runtime
                .resolve_tree_ref(tree_ref)
                .expect("missing disowned repo tree");
            assert!(real_root.join("README.md").is_file());
            assert!(real_root.join("src").is_dir());
        },
        other => panic!("expected repo tree listing, got {other:?}"),
    }

    let repo_child = harness
        .runtime
        .call_lookup_child("octocat/Hello-World", "_repo")
        .await
        .unwrap();
    match repo_child {
        OpResult::Lookup(LookupResult::Subtree(tree_ref)) => {
            let real_root = harness
                .runtime
                .resolve_tree_ref(tree_ref)
                .expect("missing disowned repo tree");
            assert!(real_root.join("README.md").is_file());
            assert!(real_root.join("src").is_dir());
            assert_eq!(
                std::fs::read(real_root.join("README.md")).unwrap(),
                b"Hello from cache\n"
            );
            assert!(real_root.join("src/main.rs").is_file());
        },
        other => panic!("expected repo child lookup, got {other:?}"),
    }
}

#[test]
fn github_provider_missing_numbered_resources_validate_on_lookup() {
    use omnifs_host::omnifs::provider::types::{
        Callout, CalloutResult, ErrorKind, Header, HttpRequest, HttpResponse,
    };

    #[allow(clippy::needless_pass_by_value)]
    fn expect_fetch(response: ProviderReturn) -> HttpRequest {
        let ProviderReturn {
            terminal: None,
            callouts,
            ..
        } = &response
        else {
            panic!("expected callouts response, got {response:?}");
        };
        let [Callout::Fetch(request)] = callouts.as_slice() else {
            panic!("expected fetch callout, got {response:?}");
        };
        request.clone()
    }

    let mut session = GithubProviderSession::new();

    let issued =
        expect_fetch(session.lookup_child(1, "octocat/Hello-World/_issues/_open", "999999999"));
    assert!(
        issued
            .url
            .ends_with("/repos/octocat/Hello-World/issues/999999999"),
        "unexpected issue lookup URL: {}",
        issued.url
    );

    let response = session.resume(
        1,
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 404,
            headers: Vec::<Header>::new(),
            body: b"{\"message\":\"Not Found\"}".to_vec(),
        })],
    );

    match response {
        ProviderReturn {
            terminal: Some(OpResult::Err(error)),
            ..
        } => {
            assert_eq!(error.kind, ErrorKind::NotFound);
        },
        other => panic!("expected lookup ProviderErr(NotFound) after 404, got {other:?}"),
    }
}

#[test]
fn github_pr_lookup_validates_and_exposes_diff() {
    use omnifs_host::omnifs::provider::types::{
        Callout, CalloutError, CalloutResult, ErrorKind, HttpRequest, HttpResponse,
    };

    #[allow(clippy::needless_pass_by_value)]
    fn expect_fetch(response: ProviderReturn) -> HttpRequest {
        let ProviderReturn {
            terminal: None,
            callouts,
            ..
        } = &response
        else {
            panic!("expected callouts response, got {response:?}");
        };
        let [Callout::Fetch(request)] = callouts.as_slice() else {
            panic!("expected fetch callout, got {response:?}");
        };
        request.clone()
    }

    let mut session = GithubProviderSession::new();

    let lookup_fetch =
        expect_fetch(session.lookup_child(70, "octocat/Hello-World/_prs/_open", "7"));
    assert!(
        lookup_fetch
            .url
            .ends_with("/repos/octocat/Hello-World/pulls/7"),
        "unexpected PR lookup URL: {}",
        lookup_fetch.url
    );

    let lookup = session.resume(
        70,
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: br#"{
                "number": 7,
                "title": "Fix the thing",
                "body": "PR body",
                "state": "open",
                "user": {"login": "octocat"}
            }"#
            .to_vec(),
        })],
    );
    match lookup {
        ProviderReturn {
            terminal: Some(OpResult::Lookup(LookupResult::Entry(result))),
            ..
        } => {
            let target = &result.target;
            assert_eq!(target.name, "7");
            assert!(matches!(target.kind, EntryKind::Directory));

            let names: Vec<&str> = result
                .siblings
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert!(
                names.contains(&"diff"),
                "lookup siblings should include diff, got {names:?}"
            );
            assert!(
                names.contains(&"comments"),
                "lookup siblings should include comments, got {names:?}"
            );
        },
        other => panic!("expected validated PR lookup result, got {other:?}"),
    }

    let read = session.read_file(70, "octocat/Hello-World/_prs/_open/7/diff");
    assert!(
        read.is_suspended(),
        "expected PR diff read to dispatch fetch, got {read:?}"
    );

    let response = session.resume(
        70,
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: b"diff --git a/file b/file\n".to_vec(),
        })],
    );

    match response {
        ProviderReturn {
            terminal: Some(OpResult::Read(file)),
            ..
        } => {
            assert_eq!(file.content, b"diff --git a/file b/file\n");
        },
        other => panic!("expected PR diff file after read, got {other:?}"),
    }

    let retry = session.read_file(71, "octocat/Hello-World/_prs/_open/7/diff");
    assert!(
        retry.is_suspended(),
        "expected PR diff reread to refetch, got {retry:?}"
    );
    let response = session.resume(
        71,
        vec![CalloutResult::CalloutError(CalloutError {
            kind: ErrorKind::Network,
            message: "network down".to_string(),
            retryable: true,
        })],
    );
    match response {
        ProviderReturn {
            terminal: Some(OpResult::Err(error)),
            ..
        } => {
            assert_eq!(error.kind, ErrorKind::Network);
        },
        other => panic!("expected Network error on refetch, got {other:?}"),
    }
}

#[test]
fn github_projected_resource_reads_return_all_fetched_siblings() {
    use omnifs_host::omnifs::provider::types::{Callout, CalloutResult, HttpRequest, HttpResponse};

    #[allow(clippy::needless_pass_by_value)]
    fn expect_fetch(response: ProviderReturn) -> HttpRequest {
        let ProviderReturn {
            terminal: None,
            callouts,
            ..
        } = &response
        else {
            panic!("expected callouts response, got {response:?}");
        };
        let [Callout::Fetch(request)] = callouts.as_slice() else {
            panic!("expected fetch callout, got {response:?}");
        };
        request.clone()
    }

    let mut session = GithubProviderSession::new();

    let pr_fetch = expect_fetch(session.read_file(72, "octocat/Hello-World/_prs/_open/7/title"));
    assert!(
        pr_fetch.url.ends_with("/repos/octocat/Hello-World/pulls/7"),
        "unexpected PR read URL: {}",
        pr_fetch.url
    );

    let pr_read = session.resume(
        72,
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: br#"{
                "number":7,
                "title":"PR title",
                "body":"PR body",
                "state":"open",
                "user":{"login":"octocat"}
            }"#
            .to_vec(),
        })],
    );
    match pr_read {
        ProviderReturn {
            terminal: Some(OpResult::Read(result)),
            ..
        } => {
            assert_eq!(result.content, b"PR title".to_vec());
            let sibling_names: Vec<&str> = result
                .sibling_files
                .iter()
                .map(|file| file.name.as_str())
                .collect();
            assert_eq!(sibling_names, vec!["body", "state", "user"]);
        },
        other => panic!("expected PR file result with sibling files, got {other:?}"),
    }

    let run_fetch =
        expect_fetch(session.read_file(73, "octocat/Hello-World/_actions/runs/123/status"));
    assert!(
        run_fetch
            .url
            .ends_with("/repos/octocat/Hello-World/actions/runs/123"),
        "unexpected action run read URL: {}",
        run_fetch.url
    );

    let run_read = session.resume(
        73,
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: br#"{"id":123,"status":"completed","conclusion":"success"}"#.to_vec(),
        })],
    );
    match run_read {
        ProviderReturn {
            terminal: Some(OpResult::Read(result)),
            ..
        } => {
            assert_eq!(result.content, b"completed".to_vec());
            let sibling_names: Vec<&str> = result
                .sibling_files
                .iter()
                .map(|file| file.name.as_str())
                .collect();
            assert_eq!(sibling_names, vec!["conclusion"]);
        },
        other => panic!("expected action run file result with sibling files, got {other:?}"),
    }
}

#[test]
fn github_provider_read_routes_dispatch_async_handlers() {
    for path in [
        "octocat/Hello-World/_issues/_open/1/title",
        "octocat/Hello-World/_prs/_open/1/diff",
        "octocat/Hello-World/_actions/runs/1/status",
    ] {
        let response = invoke_github_read_route(path);
        assert!(
            response.is_suspended(),
            "expected async effect dispatch for {path}, got {response:?}"
        );
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn github_provider_resource_reads_do_not_fall_back_to_provider_cache() {
    use omnifs_host::omnifs::provider::types::{
        CalloutError, CalloutResult, ErrorKind, Header, HttpResponse,
    };

    struct Case {
        name: &'static str,
        path: &'static str,
        ok_headers: Vec<Header>,
        ok_body: &'static [u8],
        expected_content: &'static [u8],
    }

    let cases = [
        Case {
            name: "issue title",
            path: "octocat/Hello-World/_issues/_open/1/title",
            ok_headers: vec![Header {
                name: "etag".to_string(),
                value: "\"issue-1\"".to_string(),
            }],
            ok_body: br#"{
                "number": 1,
                "title": "Cached issue title",
                "body": "Body",
                "state": "open",
                "user": {"login": "octocat"}
            }"#,
            expected_content: b"Cached issue title",
        },
        Case {
            name: "pr diff",
            path: "octocat/Hello-World/_prs/_open/7/diff",
            ok_headers: Vec::new(),
            ok_body: b"diff --git a/file b/file\n",
            expected_content: b"diff --git a/file b/file\n",
        },
        Case {
            name: "action status",
            path: "octocat/Hello-World/_actions/runs/99/status",
            ok_headers: Vec::new(),
            ok_body: br#"{"id":99,"status":"completed","conclusion":"success"}"#,
            expected_content: b"completed",
        },
    ];

    let mut session = GithubProviderSession::new();
    let mut id = 1_u64;
    for case in &cases {
        let first = session.read_file(id, case.path);
        assert!(
            first.is_suspended(),
            "{name}: expected fetch effect on first read, got {first:?}",
            name = case.name
        );
        let cached = session.resume(
            id,
            vec![CalloutResult::HttpResponse(HttpResponse {
                status: 200,
                headers: case.ok_headers.clone(),
                body: case.ok_body.to_vec(),
            })],
        );
        match cached {
            ProviderReturn {
                terminal: Some(OpResult::Read(file)),
                ..
            } => {
                assert_eq!(
                    file.content,
                    case.expected_content,
                    "{name}: unexpected cached content",
                    name = case.name
                );
            },
            other => panic!("{}: expected cached content, got {other:?}", case.name),
        }

        id += 1;
        let second = session.read_file(id, case.path);
        assert!(
            second.is_suspended(),
            "{name}: expected fetch effect on second read (no provider cache), got {second:?}",
            name = case.name
        );
        let error = session.resume(
            id,
            vec![CalloutResult::CalloutError(CalloutError {
                kind: ErrorKind::Network,
                message: "network down".to_string(),
                retryable: true,
            })],
        );
        match error {
            ProviderReturn {
                terminal: Some(OpResult::Err(err)),
                ..
            } => {
                assert_eq!(
                    err.kind,
                    ErrorKind::Network,
                    "{}: wrong error kind",
                    case.name
                );
            },
            other => panic!(
                "{}: expected Network error on second read, got {other:?}",
                case.name
            ),
        }
        id += 1;
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn github_provider_comment_routes_refetch_and_reject_zero_index() {
    use omnifs_host::omnifs::provider::types::{Callout, CalloutError, CalloutResult, ErrorKind};

    fn ok_body(body: &[u8]) -> Vec<CalloutResult> {
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: body.to_vec(),
        })]
    }

    fn network_error() -> Vec<CalloutResult> {
        vec![CalloutResult::CalloutError(CalloutError {
            kind: ErrorKind::Network,
            message: "network down".to_string(),
            retryable: true,
        })]
    }

    fn expect_network_error_on_refetch(
        session: &mut GithubProviderSession,
        id: u64,
        dispatch: impl FnOnce(&mut GithubProviderSession, u64) -> ProviderReturn,
    ) {
        let first = dispatch(session, id);
        assert!(
            first.is_suspended(),
            "expected fetch effect on refetch, got {first:?}"
        );
        match session.resume(id, network_error()) {
            ProviderReturn {
                terminal: Some(OpResult::Err(error)),
                ..
            } => {
                assert_eq!(error.kind, ErrorKind::Network);
            },
            other => panic!("expected Network error on refetch, got {other:?}"),
        }
    }

    fn expect_not_found(response: ProviderReturn) {
        match response {
            ProviderReturn {
                terminal: Some(OpResult::Err(error)),
                ..
            } => {
                assert_eq!(error.kind, ErrorKind::NotFound);
            },
            other => panic!("expected NotFound error, got {other:?}"),
        }
    }

    fn expect_fetch_url(response: ProviderReturn) -> String {
        let ProviderReturn {
            terminal: None,
            callouts,
            ..
        } = response
        else {
            panic!("expected fetch callout, got {response:?}");
        };
        let [Callout::Fetch(request)] = callouts.as_slice() else {
            panic!("expected single fetch callout, got {callouts:?}");
        };
        request.url.clone()
    }

    let mut session = GithubProviderSession::new();

    // Issue comments surface through list_children.
    let issue_list_path = "octocat/Hello-World/_issues/_open/1/comments";
    let issue_first = session.list_children(50, issue_list_path);
    assert!(issue_first.is_suspended());
    match session.resume(
        50,
        ok_body(br#"[{"user":{"login":"octocat"},"body":"first issue comment"}]"#),
    ) {
        ProviderReturn {
            terminal: Some(OpResult::List(ListResult::Entries(listing))),
            ..
        } => {
            let names: Vec<&str> = listing
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert_eq!(names, vec!["1"]);
        },
        other => panic!("expected issue comment listing, got {other:?}"),
    }
    expect_network_error_on_refetch(&mut session, 51, |s, id| {
        s.list_children(id, issue_list_path)
    });
    expect_not_found(session.read_file(52, "octocat/Hello-World/_issues/_open/1/comments/0"));

    let issue_page_two_url =
        expect_fetch_url(session.read_file(56, "octocat/Hello-World/_issues/_open/1/comments/101"));
    assert!(
        issue_page_two_url.contains("issues/1/comments?per_page=100&page=2"),
        "expected second-page issue comment fetch, got {issue_page_two_url}"
    );
    match session.resume(
        56,
        ok_body(br#"[{"user":{"login":"octocat"},"body":"page two issue comment"}]"#),
    ) {
        ProviderReturn {
            terminal: Some(OpResult::Read(file)),
            ..
        } => {
            assert_eq!(file.content, b"octocat:\npage two issue comment\n");
        },
        other => panic!("expected issue comment page-two content, got {other:?}"),
    }

    // PR comments surface through read_file at a specific index.
    let pr_read_path = "octocat/Hello-World/_prs/_open/7/comments/1";
    let pr_first = session.read_file(53, pr_read_path);
    assert!(pr_first.is_suspended());
    match session.resume(
        53,
        ok_body(br#"[{"user":{"login":"hubot"},"body":"first pr comment"}]"#),
    ) {
        ProviderReturn {
            terminal: Some(OpResult::Read(file)),
            ..
        } => {
            assert_eq!(file.content, b"hubot:\nfirst pr comment\n");
        },
        other => panic!("expected PR comment content, got {other:?}"),
    }
    expect_network_error_on_refetch(&mut session, 54, |s, id| s.read_file(id, pr_read_path));
    expect_not_found(session.read_file(55, "octocat/Hello-World/_prs/_open/7/comments/0"));

    let pr_page_two_url =
        expect_fetch_url(session.read_file(57, "octocat/Hello-World/_prs/_open/7/comments/101"));
    assert!(
        pr_page_two_url.contains("issues/7/comments?per_page=100&page=2"),
        "expected second-page PR comment fetch, got {pr_page_two_url}"
    );
    match session.resume(
        57,
        ok_body(br#"[{"user":{"login":"hubot"},"body":"page two pr comment"}]"#),
    ) {
        ProviderReturn {
            terminal: Some(OpResult::Read(file)),
            ..
        } => {
            assert_eq!(file.content, b"hubot:\npage two pr comment\n");
        },
        other => panic!("expected PR comment page-two content, got {other:?}"),
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn github_provider_paginates_issue_and_pr_search_results() {
    use omnifs_host::omnifs::provider::types::{
        Callout, CalloutResult, Header, HttpRequest, HttpResponse,
    };

    #[allow(clippy::needless_pass_by_value)]
    fn expect_fetch(response: ProviderReturn) -> HttpRequest {
        let ProviderReturn {
            terminal: None,
            callouts,
            ..
        } = &response
        else {
            panic!("expected callouts response, got {response:?}");
        };
        let [Callout::Fetch(request)] = callouts.as_slice() else {
            panic!("expected fetch callout, got {response:?}");
        };
        request.clone()
    }

    fn search_page(first_number: u64) -> Vec<CalloutResult> {
        let body = format!(
            r#"{{
                "total_count": 150,
                "items": [{{
                    "number": {first_number},
                    "title": "page item",
                    "body": "text",
                    "state": "open",
                    "user": {{"login": "octocat"}}
                }}]
            }}"#
        );
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 200,
            headers: vec![Header {
                name: "etag".to_string(),
                value: "\"page\"".to_string(),
            }],
            body: body.into_bytes(),
        })]
    }

    struct Case {
        kind: &'static str,
        path: &'static str,
        expected_query: &'static str,
        page_one_number: u64,
        page_two_number: u64,
    }

    let cases = [
        Case {
            kind: "issue",
            path: "octocat/Hello-World/_issues/_all",
            expected_query: "/search/issues?q=repo:octocat/Hello-World+is:issue",
            page_one_number: 1,
            page_two_number: 101,
        },
        Case {
            kind: "pr",
            path: "octocat/Hello-World/_prs/_all",
            expected_query: "/search/issues?q=repo:octocat/Hello-World+is:pr",
            page_one_number: 7,
            page_two_number: 107,
        },
    ];

    let mut session = GithubProviderSession::new();
    for (index, case) in cases.iter().enumerate() {
        let id = 20 + index as u64;
        let first = expect_fetch(session.list_children(id, case.path));
        assert!(
            first.url.contains(case.expected_query),
            "{kind}: unexpected first-page URL {url}",
            kind = case.kind,
            url = first.url,
        );

        let second = expect_fetch(session.resume(id, search_page(case.page_one_number)));
        assert!(
            second.url.contains("&page=2"),
            "{kind}: expected second-page URL, got {url}",
            kind = case.kind,
            url = second.url,
        );

        let final_response = session.resume(id, search_page(case.page_two_number));
        assert!(
            final_response.callouts.is_empty(),
            "{}: terminal listing should carry no callouts, got {:?}",
            case.kind,
            final_response.callouts
        );

        match final_response {
            ProviderReturn {
                terminal: Some(OpResult::List(ListResult::Entries(listing))),
                ..
            } => {
                let names: Vec<&str> = listing
                    .entries
                    .iter()
                    .map(|entry| entry.name.as_str())
                    .collect();
                let want_first = case.page_one_number.to_string();
                let want_second = case.page_two_number.to_string();
                assert!(
                    names.contains(&want_first.as_str()),
                    "{kind}: missing {want_first} in {names:?}",
                    kind = case.kind,
                );
                assert!(
                    names.contains(&want_second.as_str()),
                    "{kind}: missing {want_second} in {names:?}",
                    kind = case.kind,
                );
            },
            other => panic!("{}: expected paginated listing, got {other:?}", case.kind),
        }
    }
}

#[test]
fn github_provider_lookup_owner_validates_and_owner_listing_classifies_with_org_fallback() {
    use omnifs_host::omnifs::provider::types::{
        Callout, CalloutResult, Header, HttpRequest, HttpResponse,
    };

    #[allow(clippy::needless_pass_by_value)]
    fn expect_fetch(response: ProviderReturn) -> HttpRequest {
        let ProviderReturn {
            terminal: None,
            callouts,
            ..
        } = &response
        else {
            panic!("expected callouts response, got {response:?}");
        };
        let [Callout::Fetch(request)] = callouts.as_slice() else {
            panic!("expected fetch callout, got {response:?}");
        };
        request.clone()
    }

    let mut session = GithubProviderSession::new();

    let first = expect_fetch(session.lookup_child(30, "", "openai"));
    assert!(
        first.url.ends_with("/users/openai"),
        "expected user profile lookup first, got {}",
        first.url
    );

    let second = expect_fetch(session.resume(
        30,
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 404,
            headers: vec![Header {
                name: "etag".to_string(),
                value: "\"miss\"".to_string(),
            }],
            body: Vec::new(),
        })],
    ));
    assert!(
        second.url.ends_with("/orgs/openai"),
        "expected org profile fallback, got {}",
        second.url
    );

    let repos_fetch = expect_fetch(session.resume(
        30,
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: br#"{
                "login": "openai",
                "public_repos": 42
            }"#
            .to_vec(),
        })],
    ));
    assert!(
        repos_fetch
            .url
            .ends_with("/orgs/openai/repos?per_page=100&sort=updated&page=1"),
        "expected repo listing fetch after owner classification, got {}",
        repos_fetch.url
    );

    let lookup = session.resume(
        30,
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: br#"[{"name":"api"}]"#.to_vec(),
        })],
    );
    match lookup {
        ProviderReturn {
            terminal: Some(OpResult::Lookup(LookupResult::Entry(result))),
            ..
        } => {
            let entry = &result.target;
            assert_eq!(entry.name, "openai");
            assert!(matches!(entry.kind, EntryKind::Directory));
            let names: Vec<&str> = result
                .siblings
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert!(
                names.contains(&"api"),
                "expected repo lookup siblings after owner classification, got {names:?}"
            );
        },
        other => panic!("expected owner lookup result, got {other:?}"),
    }

    // Root is not enumerable; should always return empty, regardless
    // of which owners have been resolved in prior calls.
    let root_listing = session.list_children(32, "");
    match root_listing {
        ProviderReturn {
            terminal: Some(OpResult::List(ListResult::Entries(listing))),
            ..
        } => {
            assert!(
                listing.entries.is_empty(),
                "root should be empty, got {:?}",
                listing.entries
            );
        },
        other => panic!("expected empty root listing, got {other:?}"),
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn github_provider_polls_events_and_invalidates_caches() {
    use omnifs_host::omnifs::provider::types::{
        ActivePathSet, Callout, CalloutError, CalloutResult, ErrorKind, Header, HttpRequest,
        HttpResponse,
    };

    #[allow(clippy::needless_pass_by_value)]
    fn expect_fetch(response: ProviderReturn) -> HttpRequest {
        let ProviderReturn {
            terminal: None,
            callouts,
            ..
        } = &response
        else {
            panic!("expected callouts response, got {response:?}");
        };
        let [Callout::Fetch(request)] = callouts.as_slice() else {
            panic!("expected fetch callout, got {response:?}");
        };
        request.clone()
    }

    fn expect_callouts(response: ProviderReturn) -> Vec<Callout> {
        let ProviderReturn {
            terminal: None,
            callouts,
            ..
        } = response
        else {
            panic!("expected callouts response, got {response:?}");
        };
        callouts
    }

    fn repo_active_path(owner: &str, repo: &str) -> ActivePathSet {
        ActivePathSet {
            mount_id: "/{owner}/{repo}".to_string(),
            mount_name: "Repo".to_string(),
            paths: vec![format!("/{owner}/{repo}")],
        }
    }

    let mut session = GithubProviderSession::new();
    let issue_path = "octocat/Hello-World/_issues/_open/1/title";

    let issue_fetch = expect_fetch(session.read_file(40, issue_path));
    assert!(
        issue_fetch
            .url
            .ends_with("/repos/octocat/Hello-World/issues/1"),
        "unexpected issue fetch URL: {}",
        issue_fetch.url
    );
    let issue_cached = session.resume(
        40,
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 200,
            headers: vec![Header {
                name: "etag".to_string(),
                value: "\"issue-1\"".to_string(),
            }],
            body: br#"{
                "number": 1,
                "title": "Cached issue title",
                "body": "Body",
                "state": "open",
                "user": {"login": "octocat"}
            }"#
            .to_vec(),
        })],
    );
    match issue_cached {
        ProviderReturn {
            terminal: Some(OpResult::Read(file)),
            ..
        } => {
            assert_eq!(file.content, b"Cached issue title");
        },
        other => panic!("expected cached issue file content, got {other:?}"),
    }

    let first_tick = expect_callouts(
        session.timer_tick_with_paths(41, vec![repo_active_path("octocat", "Hello-World")]),
    );
    assert_eq!(
        first_tick.len(),
        1,
        "unexpected first tick callouts: {first_tick:?}"
    );
    let Callout::Fetch(first_events_request) = &first_tick[0] else {
        panic!("expected first tick fetch callout, got {:?}", first_tick[0]);
    };
    assert!(
        first_events_request
            .url
            .ends_with("/repos/octocat/Hello-World/events?per_page=30"),
        "unexpected events URL: {}",
        first_events_request.url
    );

    // Invalidations now live on the event-outcome terminal rather than
    // fire-and-forget callouts.
    let first_tick_done = session.resume(
        41,
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 200,
            headers: vec![Header {
                name: "etag".to_string(),
                value: "\"events-1\"".to_string(),
            }],
            body: br#"[{"type":"IssuesEvent"}]"#.to_vec(),
        })],
    );
    assert!(
        first_tick_done.callouts.is_empty(),
        "event-outcome terminals should not carry callouts, got {:?}",
        first_tick_done.callouts
    );
    match &first_tick_done.terminal {
        Some(OpResult::Event(outcome)) => {
            assert_eq!(
                outcome.invalidate_prefixes,
                vec!["octocat/Hello-World/_issues".to_string()],
                "unexpected invalidate_prefixes: {:?}",
                outcome.invalidate_prefixes
            );
        },
        other => panic!("expected Event terminal with invalidations, got {other:?}"),
    }

    let issue_refetch = expect_fetch(session.read_file(42, issue_path));
    assert!(
        issue_refetch
            .url
            .ends_with("/repos/octocat/Hello-World/issues/1"),
        "unexpected issue refetch URL: {}",
        issue_refetch.url
    );
    let stale_after_invalidation = session.resume(
        42,
        vec![CalloutResult::CalloutError(CalloutError {
            kind: ErrorKind::Network,
            message: "network down".to_string(),
            retryable: true,
        })],
    );
    assert!(
        matches!(
            stale_after_invalidation,
            ProviderReturn {
                terminal: Some(OpResult::Err(_)),
                ..
            }
        ),
        "expected invalidated cache miss, got {stale_after_invalidation:?}"
    );

    let second_tick = expect_callouts(
        session.timer_tick_with_paths(43, vec![repo_active_path("octocat", "Hello-World")]),
    );
    assert_eq!(
        second_tick.len(),
        1,
        "unexpected second tick callouts: {second_tick:?}"
    );
    let Callout::Fetch(second_events_request) = &second_tick[0] else {
        panic!(
            "expected second tick fetch callout, got {:?}",
            second_tick[0]
        );
    };
    assert!(
        second_events_request.headers.iter().any(|header| {
            header.name.eq_ignore_ascii_case("if-none-match") && header.value == "\"events-1\""
        }),
        "missing If-None-Match header on second poll: {:?}",
        second_events_request.headers
    );
    let second_tick_done = session.resume(
        43,
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 304,
            headers: vec![Header {
                name: "etag".to_string(),
                value: "\"events-1\"".to_string(),
            }],
            body: Vec::new(),
        })],
    );
    assert!(
        matches!(
            second_tick_done,
            ProviderReturn {
                terminal: Some(OpResult::Event(_)),
                ..
            }
        ),
        "expected second timer tick event terminal, got {second_tick_done:?}"
    );
}

#[test]
fn github_provider_list_routes_preserve_typed_http_errors() {
    use omnifs_host::omnifs::provider::types::{
        Callout, CalloutResult, ErrorKind, Header, HttpRequest, HttpResponse,
    };

    #[allow(clippy::needless_pass_by_value)]
    fn expect_fetch(response: ProviderReturn) -> HttpRequest {
        let ProviderReturn {
            terminal: None,
            callouts,
            ..
        } = &response
        else {
            panic!("expected callouts response, got {response:?}");
        };
        let [Callout::Fetch(request)] = callouts.as_slice() else {
            panic!("expected fetch callout, got {response:?}");
        };
        request.clone()
    }

    fn denied_page() -> Vec<CalloutResult> {
        vec![CalloutResult::HttpResponse(HttpResponse {
            status: 403,
            headers: vec![Header {
                name: "etag".to_string(),
                value: "\"denied\"".to_string(),
            }],
            body: br#"{"message":"forbidden"}"#.to_vec(),
        })]
    }

    fn expect_denied(response: ProviderReturn) {
        let ProviderReturn {
            terminal: Some(OpResult::Err(error)),
            ..
        } = response
        else {
            panic!("expected provider error result, got {response:?}");
        };
        assert_eq!(error.kind, ErrorKind::Denied);
    }

    enum UrlCheck {
        Contains(&'static str),
        EndsWith(&'static str),
    }

    let cases = [
        (
            "issues",
            "octocat/Hello-World/_issues/_all",
            UrlCheck::Contains("/search/issues?q=repo:octocat/Hello-World+is:issue"),
        ),
        (
            "prs",
            "octocat/Hello-World/_prs/_all",
            UrlCheck::Contains("/search/issues?q=repo:octocat/Hello-World+is:pr"),
        ),
        (
            "actions",
            "octocat/Hello-World/_actions/runs",
            UrlCheck::EndsWith("/repos/octocat/Hello-World/actions/runs?per_page=30"),
        ),
    ];

    let mut session = GithubProviderSession::new();
    for (index, (kind, path, check)) in cases.into_iter().enumerate() {
        let id = 50 + index as u64;
        let fetch = expect_fetch(session.list_children(id, path));
        match check {
            UrlCheck::Contains(needle) => assert!(
                fetch.url.contains(needle),
                "{kind}: unexpected URL {}",
                fetch.url
            ),
            UrlCheck::EndsWith(suffix) => assert!(
                fetch.url.ends_with(suffix),
                "{kind}: unexpected URL {}",
                fetch.url
            ),
        }
        expect_denied(session.resume(id, denied_page()));
    }
}
