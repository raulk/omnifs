use omnifs_sdk::Cx;
use omnifs_sdk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone)]
#[omnifs_sdk::config]
struct Config;

#[derive(Clone)]
struct State;

mod root_handlers {
    use super::*;

    pub struct RootHandlers;

    #[omnifs_sdk::handlers]
    impl RootHandlers {
        #[omnifs_sdk::dir("/")]
        async fn root(_cx: &DirCx<'_, State>) -> Result<Projection> {
            Ok(Projection::new())
        }
    }
}

mod hello_handlers {
    use super::*;

    pub struct HelloHandlers;

    #[omnifs_sdk::handlers]
    impl HelloHandlers {
        #[omnifs_sdk::dir("/hello")]
        async fn hello_dir(_cx: &DirCx<'_, State>) -> Result<Projection> {
            Ok(Projection::new())
        }

        #[omnifs_sdk::file("/hello/{name}")]
        async fn hello(_cx: &Cx<State>, name: String) -> Result<FileContent> {
            Ok(FileContent::bytes(format!("hello {name}\n")))
        }
    }
}

mod extras_handlers {
    use super::*;

    pub struct ExtrasHandlers;

    #[omnifs_sdk::handlers]
    impl ExtrasHandlers {
        #[omnifs_sdk::dir("/bundle")]
        async fn bundle(_cx: &DirCx<'_, State>) -> Result<Projection> {
            let mut projection = Projection::new();
            projection.file_with_content("title", b"bundle title\n".to_vec());
            Ok(projection)
        }

        #[omnifs_sdk::subtree("/checkout")]
        async fn checkout(_cx: &Cx<State>) -> Result<SubtreeRef> {
            Ok(SubtreeRef::new(42))
        }
    }
}

#[omnifs_sdk::provider(mounts(
    crate::root_handlers::RootHandlers,
    crate::hello_handlers::HelloHandlers,
    crate::extras_handlers::ExtrasHandlers,
))]
impl TestProvider {
    fn init(_config: Config) -> (State, ProviderInfo) {
        (
            State,
            ProviderInfo {
                name: "test".into(),
                version: "0.1.0".into(),
                description: "test provider".into(),
            },
        )
    }

    fn capabilities() -> RequestedCapabilities {
        RequestedCapabilities {
            domains: Vec::new(),
            auth_types: Vec::new(),
            max_memory_mb: 16,
            needs_git: false,
            needs_websocket: false,
            needs_streaming: false,
            refresh_interval_secs: 0,
        }
    }
}

#[tokio::test]
async fn registry_uses_path_first_handlers() {
    use omnifs_sdk::browse::{List, Lookup};

    let mut registry = omnifs_sdk::__internal::MountRegistry::new();
    root_handlers::RootHandlers::mount(&mut registry);
    hello_handlers::HelloHandlers::mount(&mut registry);
    extras_handlers::ExtrasHandlers::mount(&mut registry);
    registry.validate().unwrap();

    let cx = Cx::new(7, Rc::new(RefCell::new(State)));
    let list = registry.list_children(&cx, "/").await.unwrap();
    let List::Entries(listing) = list else {
        panic!("expected entries, got subtree");
    };
    assert!(
        listing
            .entries()
            .iter()
            .any(|entry| entry.name() == "hello")
    );

    let lookup = registry.lookup_child(&cx, "/hello", "world").await.unwrap();
    let Lookup::Entry(entry) = &lookup else {
        panic!("expected lookup entry, got {lookup:?}");
    };
    assert_eq!(entry.target().name(), "world");

    let file = registry.read_file(&cx, "/hello/world").await.unwrap();
    assert_eq!(file.content(), b"hello world\n");

    let projected = registry.read_file(&cx, "/bundle/title").await.unwrap();
    assert_eq!(projected.content(), b"bundle title\n");

    let checkout_list = registry.list_children(&cx, "/checkout").await.unwrap();
    assert!(matches!(checkout_list, List::Subtree(42)));

    let checkout_lookup = registry.lookup_child(&cx, "/", "checkout").await.unwrap();
    assert!(matches!(checkout_lookup, Lookup::Subtree(42)));
}

fn parse_unit(path: &str) -> Option<Box<dyn std::any::Any>> {
    if path.is_empty() {
        None
    } else {
        Some(Box::new(()))
    }
}

fn call_dir<'a>(
    _cx: &'a Cx<State>,
    _path: Box<dyn std::any::Any>,
    _intent: DirIntent<'a>,
) -> omnifs_sdk::handler::BoxFuture<'a, Projection> {
    Box::pin(async { Ok(Projection::new()) })
}

#[test]
fn registry_rejects_ambiguous_dir_routes() {
    let mut registry = omnifs_sdk::__internal::MountRegistry::<State>::new();
    registry
        .add_dir("/items/{id}", parse_unit, call_dir)
        .unwrap();
    registry
        .add_dir("/items/{name}", parse_unit, call_dir)
        .unwrap();

    let error = registry.validate().unwrap_err();
    assert!(error.message().contains("ambiguous dir handlers"));
}
