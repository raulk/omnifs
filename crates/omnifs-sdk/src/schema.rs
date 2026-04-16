//! JSON Schema helpers for provider config types.

pub fn json_schema_for<T>() -> Option<String>
where
    T: schemars::JsonSchema,
{
    let schema = schemars::schema_for!(T);
    serde_json::to_string(&schema).ok()
}
