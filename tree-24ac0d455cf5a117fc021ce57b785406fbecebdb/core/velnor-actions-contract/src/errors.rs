//! Stack-neutral error types for generator contracts.

use thiserror::Error;

/// Errors for contract identity, canonicalization, and schema validation.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContractError {
    /// A derived or supplied identity string failed grammar validation.
    #[error("invalid {field}: {problem}")]
    InvalidIdentity {
        /// Identity field name (e.g. `task_id`, `matrix_key`).
        field: &'static str,
        /// Machine-readable problem code plus detail.
        problem: String,
    },
    /// Canonical JSON serialization failed.
    #[error("canonical json error: {0}")]
    CanonicalJson(String),
    /// A schema-versioned document used an unsupported version.
    #[error("unsupported_schema: {field} version {found}, expected {expected}")]
    UnsupportedSchema {
        /// Document field carrying the version.
        field: &'static str,
        /// Version found in the document.
        found: String,
        /// Version this contract accepts.
        expected: &'static str,
    },
    /// Configuration validation failed for one key path.
    #[error("{file}: {key_path}: {problem}")]
    Config {
        /// Config file path as reported to the user.
        file: String,
        /// Dotted key path within the file.
        key_path: String,
        /// Machine-readable problem code plus detail.
        problem: String,
    },
    /// Two distinct inputs produced the same derived key.
    #[error("key collision: {0}")]
    Collision(String),
    /// An untrusted document exceeds the caller's size bound.
    #[error("document_too_large: {size} bytes exceeds limit {limit}")]
    DocumentTooLarge {
        /// Observed document size in bytes.
        size: usize,
        /// Enforced limit in bytes.
        limit: usize,
    },
    /// An untrusted JSON document nests deeper than the depth budget.
    #[error("json_too_deep: depth {depth} exceeds limit {limit}")]
    DocumentTooDeep {
        /// Observed nesting depth in containers.
        depth: usize,
        /// Enforced nesting budget in containers.
        limit: usize,
    },
}

impl ContractError {
    /// Build an identity validation error.
    ///
    /// The problem is sanitized ([`sanitize_error_detail`]): fail-closed
    /// errors embed untrusted values (`unknown_conclusion:{value}`),
    /// and raw newlines would let a hostile value forge log lines.
    #[must_use]
    pub fn identity(field: &'static str, problem: impl Into<String>) -> Self {
        Self::InvalidIdentity {
            field,
            problem: sanitize_error_detail(&problem.into()),
        }
    }

    /// Build an unknown-key error (`unknown_config_field`, arch §3).
    #[must_use]
    pub fn unknown_config_field(file: impl Into<String>, key_path: impl Into<String>) -> Self {
        Self::config(file, key_path, "unknown_config_field")
    }

    /// Map a serde/TOML decode failure to a key-path error (arch §3).
    ///
    /// Unknown-field prose (backtick or single-quote form, including
    /// multi-line TOML errors) becomes `unknown_config_field` with the
    /// offending key path; anything else becomes a `document` error.
    /// Pure string mapping: no IO, no parsing beyond the message.
    #[must_use]
    pub fn map_decode_error(file: impl Into<String>, message: &str) -> Self {
        let file = file.into();
        if let Some(key) = unknown_field_key(message) {
            return Self::unknown_config_field(file, key);
        }
        Self::config(file, "document", single_line(message))
    }

    /// Build a config validation error.
    ///
    /// The problem is sanitized like [`Self::identity`]: config values
    /// (`bad_target:{target}`) are attacker-adjacent in PR flows.
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

/// Strip log-forging content from one embedded error detail.
///
/// Removes ASCII control characters (including CR/LF/TAB) and caps
/// the detail at 120 chars so a hostile value renders as one
/// truncated line. Callers embedding untrusted values in
/// non-`ContractError` problems (orchestrator/renderer errors) must
/// route the value through this helper.
#[must_use]
pub fn sanitize_error_detail(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_ascii_control())
        .take(120)
        .collect()
}

/// Extract the offending key from unknown-field prose, if present.
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

/// Collapse a multi-line decode message to one line.
fn single_line(message: &str) -> String {
    message
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(300)
        .collect()
}

#[cfg(test)]
mod tests;
