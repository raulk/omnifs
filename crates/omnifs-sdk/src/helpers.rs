//! Common response helpers for providers.

use crate::error::ProviderError;
use crate::omnifs::provider::types::{Callout, OpResult, ProviderReturn};

impl ProviderReturn {
    /// Terminal return with no trailing callouts.
    pub fn terminal(op: OpResult) -> Self {
        Self {
            callouts: Vec::new(),
            terminal: Some(op),
        }
    }

    /// Suspension: callouts to run before the host calls `resume`.
    pub fn suspend(callouts: Vec<Callout>) -> Self {
        Self {
            callouts,
            terminal: None,
        }
    }

    /// Terminal with trailing callouts (rare). Host runs the callouts,
    /// discards their results, and returns the terminal.
    pub fn terminal_with_callouts(op: OpResult, callouts: Vec<Callout>) -> Self {
        Self {
            callouts,
            terminal: Some(op),
        }
    }
}

/// Build a terminal provider return carrying the given error.
pub fn err(error: impl Into<ProviderError>) -> ProviderReturn {
    ProviderReturn::terminal(OpResult::from(error.into()))
}
