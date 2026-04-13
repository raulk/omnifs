//! omnifs-host: Host runtime for the omnifs virtual filesystem.
//!
//! This crate provides the infrastructure for running WASM-based filesystem
//! providers via the WebAssembly Component Model. Key components include:
//!
//! - `registry`: Provider loading and lifecycle management
//! - `runtime`: Effect execution (HTTP, Git, KV operations)
//! - `fuse`: Linux FUSE filesystem implementation
//! - `auth`: Authentication and credential injection
//! - `config`: Instance configuration and schema validation

pub mod auth;
pub mod cache;
pub mod config;
#[cfg(target_os = "linux")]
pub mod fuse;
#[cfg(target_os = "linux")]
pub mod mount;
pub mod registry;
pub mod runtime;

wasmtime::component::bindgen!({
    path: "../../wit",
    world: "provider",
});
