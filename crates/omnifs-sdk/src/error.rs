use crate::omnifs::provider::types::{
    ActionResult, EffectError, ErrorKind, ProviderError as WitProviderError, ProviderResponse,
};
use std::fmt;

/// Provider-side error that can be converted into WIT `ActionResult::ProviderErr`.
#[derive(Clone, Debug)]
pub struct ProviderError {
    kind: ProviderErrorKind,
    message: String,
    retryable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderErrorKind {
    NotFound,
    Network,
    Timeout,
    Denied,
    InvalidInput,
    RateLimited,
    Internal,
}

impl ProviderError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: ProviderErrorKind::Internal,
            message: message.into(),
            retryable: false,
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            kind: ProviderErrorKind::NotFound,
            message: message.into(),
            retryable: false,
        }
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self {
            kind: ProviderErrorKind::InvalidInput,
            message: message.into(),
            retryable: false,
        }
    }

    pub fn network(message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind: ProviderErrorKind::Network,
            message: message.into(),
            retryable,
        }
    }

    pub fn timeout(message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind: ProviderErrorKind::Timeout,
            message: message.into(),
            retryable,
        }
    }

    pub fn denied(message: impl Into<String>) -> Self {
        Self {
            kind: ProviderErrorKind::Denied,
            message: message.into(),
            retryable: false,
        }
    }

    pub fn rate_limited(message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind: ProviderErrorKind::RateLimited,
            message: message.into(),
            retryable,
        }
    }

    pub fn from_http_status(status: u16) -> Self {
        match status {
            408 => Self::timeout(format!("HTTP {status}"), true),
            429 => Self::rate_limited(format!("HTTP {status}"), true),
            400..=499 => Self::invalid_input(format!("HTTP {status}")),
            500..=599 => Self::network(format!("HTTP {status}"), true),
            _ => Self::internal(format!("HTTP {status}")),
        }
    }

    fn kind_tag(&self) -> &'static str {
        match self.kind {
            ProviderErrorKind::NotFound => "not-found",
            ProviderErrorKind::Network => "network",
            ProviderErrorKind::Timeout => "timeout",
            ProviderErrorKind::Denied => "denied",
            ProviderErrorKind::InvalidInput => "invalid-input",
            ProviderErrorKind::RateLimited => "rate-limited",
            ProviderErrorKind::Internal => "internal",
        }
    }

    fn wit_kind(&self) -> ErrorKind {
        match self.kind {
            ProviderErrorKind::NotFound => ErrorKind::NotFound,
            ProviderErrorKind::Network => ErrorKind::Network,
            ProviderErrorKind::Timeout => ErrorKind::Timeout,
            ProviderErrorKind::Denied => ErrorKind::Denied,
            ProviderErrorKind::RateLimited => ErrorKind::RateLimited,
            ProviderErrorKind::InvalidInput => ErrorKind::VersionMismatch,
            ProviderErrorKind::Internal => ErrorKind::Internal,
        }
    }

    pub fn from_effect_error(error: &EffectError) -> Self {
        let message = format!("effect error: {}", error.message);
        match error.kind {
            ErrorKind::NotFound => Self::not_found(message),
            ErrorKind::Network => Self::network(message, error.retryable),
            ErrorKind::Timeout => Self::timeout(message, error.retryable),
            ErrorKind::Denied => Self::denied(message),
            ErrorKind::RateLimited => Self::rate_limited(message, error.retryable),
            ErrorKind::VersionMismatch => {
                Self::invalid_input(format!("effect error: {}", error.message))
            }
            ErrorKind::Internal => Self::internal(format!("effect error: {}", error.message)),
        }
    }

    pub fn is_retryable(&self) -> bool {
        self.retryable
    }

    pub fn kind(&self) -> ProviderErrorKind {
        self.kind.clone()
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = if matches!(self.kind, ProviderErrorKind::Internal) {
            self.message.clone()
        } else {
            format!(
                "[{}; retryable={}] {}",
                self.kind_tag(),
                self.retryable,
                self.message
            )
        };
        f.write_str(&message)
    }
}

impl From<ProviderError> for ActionResult {
    fn from(error: ProviderError) -> Self {
        ActionResult::ProviderErr(WitProviderError {
            kind: error.wit_kind(),
            message: error.message,
            retryable: error.retryable,
        })
    }
}

impl From<ProviderError> for ProviderResponse {
    fn from(error: ProviderError) -> Self {
        ProviderResponse::Done(ActionResult::from(error))
    }
}

impl From<&str> for ProviderError {
    fn from(message: &str) -> Self {
        Self::internal(message)
    }
}

impl From<&String> for ProviderError {
    fn from(message: &String) -> Self {
        Self::internal(message.as_str())
    }
}

impl From<String> for ProviderError {
    fn from(message: String) -> Self {
        Self::internal(message)
    }
}

impl From<EffectError> for ProviderError {
    fn from(error: EffectError) -> Self {
        Self::from_effect_error(&error)
    }
}
