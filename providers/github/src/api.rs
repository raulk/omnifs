//! GitHub API request building and response parsing.
//!
//! Helper functions to construct GitHub API requests and parse JSON responses.

use omnifs_sdk::prelude::*;

/// Build an HTTP GET request for the GitHub API.
pub fn github_get(path: &str) -> SingleEffect {
    let url = format!("https://api.github.com{path}");
    SingleEffect::Fetch(HttpRequest {
        method: "GET".to_string(),
        url,
        headers: vec![
            Header {
                name: "Accept".to_string(),
                value: "application/vnd.github+json".to_string(),
            },
            Header {
                name: "X-GitHub-Api-Version".to_string(),
                value: "2022-11-28".to_string(),
            },
        ],
        body: None,
    })
}

/// Parse a JSON API response body, returning the parsed value or an error string.
pub fn parse_json(body: &[u8]) -> Result<serde_json::Value, ProviderError> {
    serde_json::from_slice(body)
        .map_err(|e| ProviderError::invalid_input(format!("JSON parse error: {e}")))
}
