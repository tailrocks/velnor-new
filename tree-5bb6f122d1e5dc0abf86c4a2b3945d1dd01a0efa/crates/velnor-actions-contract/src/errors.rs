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
}

impl ContractError {
    /// Build an identity validation error.
    #[must_use]
    pub fn identity(field: &'static str, problem: impl Into<String>) -> Self {
        Self::InvalidIdentity {
            field,
            problem: problem.into(),
        }
    }

    /// Build a config validation error.
    #[must_use]
    pub fn config(
        file: impl Into<String>,
        key_path: impl Into<String>,
        problem: impl Into<String>,
    ) -> Self {
        Self::Config {
            file: file.into(),
            key_path: key_path.into(),
            problem: problem.into(),
        }
    }
}
