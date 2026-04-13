//! Authentication and credential injection for HTTP requests.
//!
//! The `AuthManager` supports bearer tokens and API key headers,
//! with optional domain filtering for multi-tenant configurations.

use crate::config::AuthConfig;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("{0}")]
    CredentialSourceMissing(String),
    #[error("unsupported auth type: {0}")]
    UnsupportedType(String),
}

/// Manages authentication header injection for HTTP requests.
///
/// Supports bearer tokens and API key headers with optional domain filtering.
pub struct AuthManager {
    injectors: Vec<AuthInjector>,
}

struct AuthInjector {
    domain: Option<String>,
    header_name: String,
    header_value: Option<String>,
}

impl AuthManager {
    pub fn none() -> Self {
        Self { injectors: vec![] }
    }

    pub fn from_configs(configs: &[AuthConfig]) -> Result<Self, AuthError> {
        let mut injectors = Vec::new();
        for config in configs {
            injectors.push(Self::build_injector(config)?);
        }
        Ok(Self { injectors })
    }

    pub fn from_config(config: &AuthConfig) -> Result<Self, AuthError> {
        Ok(Self {
            injectors: vec![Self::build_injector(config)?],
        })
    }

    fn build_injector(config: &AuthConfig) -> Result<AuthInjector, AuthError> {
        match config.auth_type.as_str() {
            "bearer-token" => {
                let token = read_credential(config).ok_or_else(|| {
                    AuthError::CredentialSourceMissing(
                        "token_env or token_file required for bearer-token".to_string(),
                    )
                })?;
                Ok(AuthInjector {
                    domain: config.domain.clone(),
                    header_name: "Authorization".to_string(),
                    header_value: token.map(|token| format!("Bearer {token}")),
                })
            }
            "api-key-header" => {
                let key = read_credential(config).ok_or_else(|| {
                    AuthError::CredentialSourceMissing(
                        "token_env or token_file required for api-key-header".to_string(),
                    )
                })?;
                let header = config.header.as_deref().unwrap_or("X-API-Key");
                Ok(AuthInjector {
                    domain: config.domain.clone(),
                    header_name: header.to_string(),
                    header_value: key,
                })
            }
            other => Err(AuthError::UnsupportedType(other.to_string())),
        }
    }

    pub fn headers_for_url(&self, url: &str) -> Vec<(String, String)> {
        let host = url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(String::from));
        self.injectors
            .iter()
            .filter(|inj| match (&inj.domain, &host) {
                (Some(d), Some(h)) => d == h,
                (None, _) => true,
                _ => false,
            })
            .filter_map(|inj| Some((inj.header_name.clone(), inj.header_value.as_ref()?.clone())))
            .collect()
    }

    pub fn requires_auth_for_url(&self, url: &str) -> bool {
        let host = url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(String::from));
        self.injectors.iter().any(|inj| match (&inj.domain, &host) {
            (Some(d), Some(h)) => d == h,
            (None, _) => true,
            _ => false,
        })
    }
}

fn read_credential(config: &AuthConfig) -> Option<Option<String>> {
    if let Some(path) = config.token_file.as_deref() {
        let token = std::fs::read_to_string(path)
            .ok()
            .map(|contents| contents.trim().to_string())
            .filter(|contents| !contents.is_empty());
        return Some(token);
    }

    config
        .token_env
        .as_deref()
        .map(|env_var| std::env::var(env_var).ok())
}
