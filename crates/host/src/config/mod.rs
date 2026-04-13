//! Instance configuration parsing and validation.
//!
//! Defines `InstanceConfig` for provider instantiation, including
//! plugin path, mount point, authentication, and capability grants.

pub mod schema;

use serde::Deserialize;

/// Configuration for a provider instance.
///
/// Loaded from TOML files in the providers configuration directory.
#[derive(Debug, Clone, Deserialize)]
pub struct InstanceConfig {
    pub plugin: String,
    pub mount: String,
    #[serde(default)]
    pub root_mount: bool,
    #[serde(default, deserialize_with = "deserialize_auth")]
    pub auth: Vec<AuthConfig>,
    pub capabilities: Option<CapabilitiesConfig>,
    #[serde(rename = "config")]
    pub config_raw: Option<toml::Value>,
}

/// Accepts both `[auth]` (single table) and `[[auth]]` (array of tables).
fn deserialize_auth<'de, D>(deserializer: D) -> Result<Vec<AuthConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(AuthConfig),
        Many(Vec<AuthConfig>),
    }
    match Option::<OneOrMany>::deserialize(deserializer)? {
        None => Ok(Vec::new()),
        Some(OneOrMany::One(single)) => Ok(vec![single]),
        Some(OneOrMany::Many(vec)) => Ok(vec),
    }
}

/// Authentication configuration for HTTP requests.
///
/// Supports bearer-token and api-key-header authentication types.
#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    #[serde(rename = "type")]
    pub auth_type: String,
    pub token_env: Option<String>,
    pub token_file: Option<String>,
    pub domain: Option<String>,
    pub header: Option<String>,
    pub scopes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CapabilitiesConfig {
    pub domains: Option<Vec<String>>,
    pub git_repos: Option<Vec<String>>,
    pub max_memory_mb: Option<u32>,
}

impl InstanceConfig {
    pub fn parse(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    pub fn from_file(path: &std::path::Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::ReadFailed(path.display().to_string(), e))?;
        Self::parse(&content).map_err(|e| ConfigError::ParseFailed(path.display().to_string(), e))
    }

    pub fn config_bytes(&self) -> Vec<u8> {
        match &self.config_raw {
            Some(value) => toml::to_string(value).unwrap_or_default().into_bytes(),
            None => Vec::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file {0}: {1}")]
    ReadFailed(String, std::io::Error),
    #[error("failed to parse config file {0}: {1}")]
    ParseFailed(String, toml::de::Error),
}
