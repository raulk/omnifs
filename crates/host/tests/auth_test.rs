#![allow(unsafe_code)]

use omnifs_host::auth::AuthManager;
use omnifs_host::config::AuthConfig;

#[test]
fn test_bearer_token_injection() {
    let auth = AuthConfig {
        auth_type: "bearer-token".to_string(),
        token_env: Some("OMNIFS_TEST_TOKEN_AUTH".to_string()),
        token_file: None,
        domain: None,
        header: None,
        scopes: None,
    };
    // SAFETY for test isolation: unique env var name avoids cross-test interference.
    unsafe { std::env::set_var("OMNIFS_TEST_TOKEN_AUTH", "ghp_test123") };
    let manager = AuthManager::from_config(&auth).unwrap();
    let headers = manager.headers_for_url("https://api.github.com/repos");
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].0, "Authorization");
    assert_eq!(headers[0].1, "Bearer ghp_test123");
    unsafe { std::env::remove_var("OMNIFS_TEST_TOKEN_AUTH") };
}

#[test]
fn test_no_injection_without_config() {
    let manager = AuthManager::none();
    let headers = manager.headers_for_url("https://api.github.com/repos");
    assert!(headers.is_empty());
}

#[test]
fn test_missing_env_var_returns_no_headers() {
    let auth = AuthConfig {
        auth_type: "bearer-token".to_string(),
        token_env: Some("DEFINITELY_NOT_SET_12345".to_string()),
        token_file: None,
        domain: None,
        header: None,
        scopes: None,
    };
    let manager = AuthManager::from_config(&auth).unwrap();
    let headers = manager.headers_for_url("https://api.github.com/repos");
    assert!(headers.is_empty());
    assert!(manager.requires_auth_for_url("https://api.github.com/repos"));
}

#[test]
fn test_bearer_token_injection_from_file() {
    let dir = tempfile::tempdir().unwrap();
    let token_file = dir.path().join("github_token");
    std::fs::write(&token_file, "ghp_file_token\n").unwrap();
    let auth = AuthConfig {
        auth_type: "bearer-token".to_string(),
        token_env: None,
        token_file: Some(token_file.display().to_string()),
        domain: None,
        header: None,
        scopes: None,
    };
    let manager = AuthManager::from_config(&auth).unwrap();
    let headers = manager.headers_for_url("https://api.github.com/repos");
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].0, "Authorization");
    assert_eq!(headers[0].1, "Bearer ghp_file_token");
}

#[test]
fn test_token_file_takes_precedence_over_env() {
    let dir = tempfile::tempdir().unwrap();
    let token_file = dir.path().join("github_token");
    std::fs::write(&token_file, "ghp_from_file").unwrap();
    let auth = AuthConfig {
        auth_type: "bearer-token".to_string(),
        token_env: Some("OMNIFS_TEST_TOKEN_AUTH_PREFERRED".to_string()),
        token_file: Some(token_file.display().to_string()),
        domain: None,
        header: None,
        scopes: None,
    };
    unsafe { std::env::set_var("OMNIFS_TEST_TOKEN_AUTH_PREFERRED", "ghp_from_env") };
    let manager = AuthManager::from_config(&auth).unwrap();
    let headers = manager.headers_for_url("https://api.github.com/repos");
    assert_eq!(headers[0].1, "Bearer ghp_from_file");
    unsafe { std::env::remove_var("OMNIFS_TEST_TOKEN_AUTH_PREFERRED") };
}
