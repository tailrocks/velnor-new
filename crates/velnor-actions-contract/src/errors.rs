use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContractError {
    #[error("invalid {field}: {problem}")]
    InvalidIdentity {
        field: &'static str,

        problem: String,
    },

    #[error("canonical json error: {0}")]
    CanonicalJson(String),

    #[error("unsupported_schema: {field} version {found}, expected {expected}")]
    UnsupportedSchema {
        field: &'static str,

        found: String,

        expected: &'static str,
    },

    #[error("{file}: {key_path}: {problem}")]
    Config {
        file: String,

        key_path: String,

        problem: String,
    },

    #[error("key collision: {0}")]
    Collision(String),

    #[error("document_too_large: {size} bytes exceeds limit {limit}")]
    DocumentTooLarge { size: usize, limit: usize },

    #[error("json_too_deep: depth {depth} exceeds limit {limit}")]
    DocumentTooDeep { depth: usize, limit: usize },
}

impl ContractError {
    #[must_use]
    pub fn identity(field: &'static str, problem: impl Into<String>) -> Self {
        Self::InvalidIdentity {
            field,
            problem: sanitize_error_detail(&problem.into()),
        }
    }

    #[must_use]
    pub fn unknown_config_field(file: impl Into<String>, key_path: impl Into<String>) -> Self {
        Self::config(file, key_path, "unknown_config_field")
    }

    #[must_use]
    pub fn map_decode_error(file: impl Into<String>, message: &str) -> Self {
        let file = file.into();
        if let Some(key) = unknown_field_key(message) {
            return Self::unknown_config_field(file, key);
        }
        Self::config(file, "document", single_line(message))
    }

    #[must_use]
    pub fn config(
        file: impl Into<String>,
        key_path: impl Into<String>,
        problem: impl Into<String>,
    ) -> Self {
        Self::Config {
            file: file.into(),
            key_path: key_path.into(),
            problem: sanitize_error_detail(&problem.into()),
        }
    }
}

#[must_use]
pub fn sanitize_error_detail(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_ascii_control())
        .take(120)
        .collect()
}

fn unknown_field_key(message: &str) -> Option<String> {
    let marker = message.find("unknown field")?;
    let rest = &message[marker + "unknown field".len()..];
    for (open, close) in [('`', '`'), ('\'', '\''), ('"', '"')] {
        if let Some(start) = rest.find(open) {
            let after = &rest[start + open.len_utf8()..];
            if let Some(end) = after.find(close) {
                let key = after[..end].trim().to_owned();
                if !key.is_empty() {
                    return Some(key);
                }
            }
        }
    }
    None
}

fn single_line(message: &str) -> String {
    message
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(300)
        .collect()
}
