//! Effect request/response types and HTTP executor.
//!
//! Defines the internal effect protocol between the host and providers,
//! including HTTP fetch, KV operations, and Git effects.

use crate::auth::AuthManager;
use crate::runtime::capability::CapabilityChecker;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Network,
    Timeout,
    Denied,
    NotFound,
    RateLimited,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitEntryKind {
    Blob,
    Tree,
    Commit,
}

#[derive(Debug, Clone)]
pub enum EffectResponse {
    HttpResponse {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    KvValue(Option<Vec<u8>>),
    KvOk,
    KvKeys(Vec<String>),
    GitRepoOpened(u64),
    GitTreeEntries(Vec<GitTreeEntryData>),
    GitBlobData(Vec<u8>),
    GitRef(String),
    GitCachedRepos(Vec<GitCachedRepoData>),
    Error {
        kind: ErrorKind,
        message: String,
        retryable: bool,
    },
}

#[derive(Debug, Clone)]
pub struct GitTreeEntryData {
    pub name: String,
    pub mode: u32,
    pub oid: String,
    pub kind: GitEntryKind,
}

#[derive(Debug, Clone)]
pub struct GitCachedRepoData {
    pub cache_key: String,
}

pub(crate) struct HttpExecutor {
    client: reqwest::Client,
    auth: Arc<AuthManager>,
    capability: Arc<CapabilityChecker>,
}

impl HttpExecutor {
    pub fn new(auth: Arc<AuthManager>, capability: Arc<CapabilityChecker>) -> Self {
        // Redirects disabled: providers must not be able to bypass the
        // capability checker by fetching an allowed URL that 302s to a
        // private/link-local address.
        let client = reqwest::Client::builder()
            .user_agent("omnifs")
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("reqwest client");
        Self {
            client,
            auth,
            capability,
        }
    }

    pub async fn execute_fetch(
        &self,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: Option<&[u8]>,
    ) -> EffectResponse {
        if let Err(e) = self.capability.check_url(url) {
            return EffectResponse::Error {
                kind: ErrorKind::Denied,
                message: e.to_string(),
                retryable: false,
            };
        }

        let auth_headers = self.auth.headers_for_url(url);
        if auth_headers.is_empty() && self.auth.requires_auth_for_url(url) {
            return EffectResponse::HttpResponse {
                status: 401,
                headers: Vec::new(),
                body: Vec::new(),
            };
        }

        let reqwest_method = match method {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "PATCH" => reqwest::Method::PATCH,
            "DELETE" => reqwest::Method::DELETE,
            "HEAD" => reqwest::Method::HEAD,
            "OPTIONS" => reqwest::Method::OPTIONS,
            other => {
                return EffectResponse::Error {
                    kind: ErrorKind::Denied,
                    message: format!("unsupported HTTP method: {other}"),
                    retryable: false,
                };
            }
        };

        let mut req = self.client.request(reqwest_method, url);

        for (name, value) in &auth_headers {
            req = req.header(name.as_str(), value.as_str());
        }
        for (name, value) in headers {
            req = req.header(name.as_str(), value.as_str());
        }
        if let Some(body) = body {
            req = req.body(body.to_vec());
        }

        match req.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let resp_headers: Vec<(String, String)> = response
                    .headers()
                    .iter()
                    .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
                    .collect();
                match response.bytes().await {
                    Ok(body) => EffectResponse::HttpResponse {
                        status,
                        headers: resp_headers,
                        body: body.to_vec(),
                    },
                    Err(e) => EffectResponse::Error {
                        kind: ErrorKind::Network,
                        message: e.to_string(),
                        retryable: true,
                    },
                }
            }
            Err(e) => EffectResponse::Error {
                kind: ErrorKind::Network,
                message: e.to_string(),
                retryable: true,
            },
        }
    }
}

pub(crate) struct MemoryKvExecutor {
    store: parking_lot::Mutex<HashMap<String, Vec<u8>>>,
}

impl MemoryKvExecutor {
    pub fn new() -> Self {
        Self {
            store: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.store.lock().get(key).cloned()
    }

    pub fn set(&self, key: &str, value: Vec<u8>) {
        self.store.lock().insert(key.to_string(), value);
    }

    pub fn delete(&self, key: &str) -> bool {
        self.store.lock().remove(key).is_some()
    }

    pub fn list_keys(&self, prefix: &str) -> Vec<String> {
        self.store
            .lock()
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect()
    }
}

impl Default for MemoryKvExecutor {
    fn default() -> Self {
        Self::new()
    }
}
