use omnifs_host::config::schema::{SchemaField, validate_config};

#[test]
fn test_valid_config_passes() {
    let schema = vec![SchemaField {
        name: "issue_format".into(),
        field_type: "string".into(),
        required: true,
        default_value: None,
        description: String::new(),
    }];
    let config: toml::Value = toml::from_str(r#"issue_format = "markdown""#).unwrap();
    assert!(validate_config(&schema, &config).is_ok());
}

#[test]
fn test_missing_required_field_fails() {
    let schema = vec![SchemaField {
        name: "issue_format".into(),
        field_type: "string".into(),
        required: true,
        default_value: None,
        description: String::new(),
    }];
    let config: toml::Value = toml::from_str(r#"other = "value""#).unwrap();
    let result = validate_config(&schema, &config);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("issue_format"));
}

#[test]
fn test_unknown_field_warns_with_suggestion() {
    let schema = vec![SchemaField {
        name: "issue_format".into(),
        field_type: "string".into(),
        required: false,
        default_value: Some("granular".into()),
        description: String::new(),
    }];
    let config: toml::Value = toml::from_str(r#"isue_format = "markdown""#).unwrap();
    let result = validate_config(&schema, &config);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("issue_format"));
}
