//! Single-import module for providers: `use omnifs_sdk::prelude::*;`

pub use crate::Op;
pub use crate::cache::Cache;
pub use crate::error::{ProviderError, ProviderErrorKind};
pub use crate::helpers::{
    dir_entry, dir_only, dir_only_no_read, dir_only_with, err, file_entry, file_only,
    file_only_with, mk_dir, mk_file,
};
pub use crate::http::{extract_effect_body, extract_http_body};

// Proc macros (invoked as #[omnifs_sdk::provider] and #[route("...")])
pub use omnifs_sdk_macros::{config, provider, route};

// Common deps re-exported so providers don't need direct dependencies
pub use hashbrown::HashMap;
pub use serde::Deserialize;

// WIT types generated once in the SDK, re-exported to all providers.
// Glob re-export so providers never need to reach into omnifs_sdk::omnifs::*.
pub use crate::omnifs::provider::types::*;
