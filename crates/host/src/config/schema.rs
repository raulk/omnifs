//! Config schema validation with Levenshtein-based suggestions.
//!
//! Validates provider configuration against declared schemas,
//! suggesting field name corrections for typos.

#[derive(Debug, Clone)]
pub struct SchemaField {
    pub name: String,
    pub field_type: String,
    pub required: bool,
    pub default_value: Option<String>,
    pub description: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("missing required config field: {field}")]
    MissingRequired { field: String },
    #[error("unknown config field: {field} (did you mean {suggestion}?)")]
    UnknownFieldWithSuggestion { field: String, suggestion: String },
    #[error("unknown config field: {field}")]
    UnknownField { field: String },
}

pub fn validate_config(schema: &[SchemaField], config: &toml::Value) -> Result<(), SchemaError> {
    let empty = toml::map::Map::new();
    let table = config.as_table().unwrap_or(&empty);
    let known_names: Vec<&str> = schema.iter().map(|f| f.name.as_str()).collect();

    for field in schema {
        if field.required && !table.contains_key(&field.name) {
            return Err(SchemaError::MissingRequired {
                field: field.name.clone(),
            });
        }
    }

    for key in table.keys() {
        if !known_names.contains(&key.as_str()) {
            let suggestion = find_closest(key, &known_names);
            return match suggestion {
                Some(s) => Err(SchemaError::UnknownFieldWithSuggestion {
                    field: key.clone(),
                    suggestion: s.to_string(),
                }),
                None => Err(SchemaError::UnknownField { field: key.clone() }),
            };
        }
    }

    Ok(())
}

fn find_closest<'a>(input: &str, candidates: &[&'a str]) -> Option<&'a str> {
    candidates
        .iter()
        .filter(|c| {
            let distance = levenshtein(input, c);
            distance <= 3 && distance > 0
        })
        .min_by_key(|c| levenshtein(input, c))
        .copied()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut matrix = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in matrix.iter_mut().enumerate().take(a.len() + 1) {
        row[0] = i;
    }
    for (j, cell) in matrix[0].iter_mut().enumerate().take(b.len() + 1) {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }
    matrix[a.len()][b.len()]
}
