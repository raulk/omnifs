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
                let header_value = read_credential(config).map(|token| format!("Bearer {token}"));
                Ok(AuthInjector {
                    domain: config.domain.clone(),
                    header_name: "Authorization".to_string(),
                    header_value,
                })
            },
            "api-key-header" => {
                let header_value = read_credential(config);
                let header = config.header.as_deref().unwrap_or("X-API-Key");
                Ok(AuthInjector {
                    domain: config.domain.clone(),
                    header_name: header.to_string(),
                    header_value,
                })
            },
            other => Err(AuthError::UnsupportedType(other.to_string())),
        }
    }

    fn injectors_for_url<'a>(&'a self, url: &str) -> impl Iterator<Item = &'a AuthInjector> + 'a {
        let host = url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(String::from));
        self.injectors
            .iter()
            .filter(move |inj| match (&inj.domain, &host) {
                (Some(d), Some(h)) => d == h,
                (None, _) => true,
                _ => false,
            })
    }

    pub fn headers_for_url(&self, url: &str) -> Vec<(String, String)> {
        self.injectors_for_url(url)
            .filter_map(|inj| Some((inj.header_name.clone(), inj.header_value.as_ref()?.clone())))
            .collect()
    }

    pub fn requires_auth_for_url(&self, url: &str) -> bool {
        self.injectors_for_url(url).next().is_some()
    }
}

fn read_credential(config: &AuthConfig) -> Option<String> {
    config
        .token_file
        .as_deref()
        .and_then(|path| {
            std::fs::read_to_string(path)
                .ok()
                .map(|contents| contents.trim().to_string())
                .filter(|contents| !contents.is_empty())
        })
        .or_else(|| {
            config
                .token_env
                .as_deref()
                .and_then(|env_var| std::env::var(env_var).ok())
                .map(|token| token.trim().to_string())
                .filter(|token| !token.is_empty())
        })
}
