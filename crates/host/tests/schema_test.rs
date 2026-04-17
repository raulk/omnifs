use omnifs_host::config::schema::validate_config;

#[test]
fn test_valid_config_passes() {
    let schema = r#"{
        "type": "object",
        "properties": {
            "issue_format": { "type": "string" }
        },
        "required": ["issue_format"],
        "additionalProperties": false
    }"#;
    let config = serde_json::json!({ "issue_format": "markdown" });
    assert!(validate_config(schema, &config).is_ok());
}

#[test]
fn test_missing_required_field_fails() {
    let schema = r#"{
        "type": "object",
        "properties": {
            "issue_format": { "type": "string" }
        },
        "required": ["issue_format"],
        "additionalProperties": false
    }"#;
    let config = serde_json::json!({ "other": "value" });
    let result = validate_config(schema, &config);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("issue_format"));
}

#[test]
fn test_unknown_field_fails() {
    let schema = r#"{
        "type": "object",
        "properties": {
            "issue_format": { "type": "string", "default": "granular" }
        },
        "additionalProperties": false
    }"#;
    let config = serde_json::json!({ "isue_format": "markdown" });
    let result = validate_config(schema, &config);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Additional properties")
    );
}
