use omnifs_host::config::InstanceConfig;

#[test]
fn test_parse_minimal_config() {
    let toml = r#"
        plugin = "test.wasm"
        mount = "test"
    "#;
    let config = InstanceConfig::parse(toml).unwrap();
    assert_eq!(config.plugin, "test.wasm");
    assert_eq!(config.mount, "test");
    assert!(config.auth.is_empty());
    assert!(config.capabilities.is_none());
    assert!(config.config_raw.is_none());
}

#[test]
fn test_parse_full_config() {
    let toml = r#"
        plugin = "github.wasm"
        mount = "github"

        [auth]
        type = "bearer-token"
        token_env = "GITHUB_TOKEN"

        [capabilities]
        domains = ["api.github.com"]
        max_memory_mb = 128

        [config]
        issue_format = "markdown"
        include_pr_diff = true
    "#;
    let config = InstanceConfig::parse(toml).unwrap();
    assert_eq!(config.plugin, "github.wasm");
    assert_eq!(config.mount, "github");
    assert_eq!(config.auth.len(), 1);
    assert!(config.capabilities.is_some());
    assert!(config.config_raw.is_some());
}

#[test]
fn test_parse_missing_required_field() {
    let toml = r#"
        mount = "test"
    "#;
    let result = InstanceConfig::parse(toml);
    assert!(result.is_err());
}

#[test]
fn test_auth_bearer_token_from_env() {
    let toml = r#"
        plugin = "test.wasm"
        mount = "test"

        [auth]
        type = "bearer-token"
        token_env = "TEST_TOKEN"
    "#;
    let config = InstanceConfig::parse(toml).unwrap();
    assert_eq!(config.auth.len(), 1);
    let auth = &config.auth[0];
    assert_eq!(auth.auth_type, "bearer-token");
    assert_eq!(auth.token_env.as_deref(), Some("TEST_TOKEN"));
}
