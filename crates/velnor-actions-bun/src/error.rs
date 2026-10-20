//! One machine-readable failure code per rejected input.

use velnor_actions_contract::sanitize_error_detail;

/// A rejected Bun input; the code is stable and safe to print.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct BunError {
    code: String,
}

impl BunError {
    /// Error with `name` and an attacker-adjacent `detail`, sanitized.
    #[must_use]
    pub fn new(name: &str, detail: &str) -> Self {
        Self {
            code: format!("{name}:{}", sanitize_error_detail(detail)),
        }
    }

    /// Error that carries no detail.
    #[must_use]
    pub fn bare(name: &str) -> Self {
        Self {
            code: name.to_owned(),
        }
    }

    /// Stable failure code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }
}
