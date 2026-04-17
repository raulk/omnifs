//! omnifs provider SDK.
//!
//! Provides WIT bindings, helper types, and proc macros for building
//! omnifs providers. Providers depend only on this crate.
//!
//! Usage: `#[omnifs_sdk::config]` on config types, `#[omnifs_sdk::provider]`
//! on an impl block, and `#[route("...")]` on path handler methods within
//! the block.

// Generate WIT bindings once; providers import from here.
wit_bindgen::generate!({
    world: "provider",
    path: "../../wit",
    pub_export_macro: true,
});

pub mod cache;
pub mod error;
pub mod helpers;
pub mod http;
pub mod prelude;
pub mod schema;

// Re-export proc macros at the crate root so #[omnifs_sdk::provider] works.
pub use omnifs_sdk_macros::Config;
pub use omnifs_sdk_macros::config;
pub use omnifs_sdk_macros::provider;
pub use omnifs_sdk_macros::route;

// Re-export deps that generated code references, so providers don't need
// direct dependencies on them.
pub use hashbrown;
pub use schemars;
pub use serde;
pub use serde_json;

/// Internal types used by generated code. Not part of the public API.
pub mod __internal {
    pub struct StateWrapper<S, C> {
        pub inner: S,
        pub pending: crate::hashbrown::HashMap<u64, C>,
    }
}

/// Filesystem operation kind, passed to route handlers.
#[derive(Clone, Copy, Debug)]
pub enum Op {
    Lookup(u64),
    List(u64),
    Read(u64),
}

impl Op {
    pub fn id(&self) -> u64 {
        match self {
            Op::Lookup(id) | Op::List(id) | Op::Read(id) => *id,
        }
    }
}
