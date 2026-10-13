//! Structured inspection findings (tool §3).
//!
//! Every finding has a stable code, path, observed value, recommended value
//! or action, and reason. Adapters emit these; the orchestrator merges them
//! into inventory, plan, and generation reports.

use serde::{Deserialize, Serialize};

use velnor_actions_contract::canonical::normalize_posix_path;
use velnor_actions_contract::errors::ContractError;

/// One structured finding (tool §3).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    /// Stable `snake_case` code (e.g. `missing_recommended_input`).
    pub code: String,
    /// Repo-relative path that supplied the value.
    pub path: String,
    /// Observed value, when one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<String>,
    /// Recommended value, when the fix is a value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended: Option<String>,
    /// Recommended action, when the fix is a procedure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// Why this finding matters.
    pub reason: String,
}

impl Finding {
    /// Validate code grammar, path shape, guidance, and reason.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if !is_stable_code(&self.code) {
            return Err(ContractError::identity("finding.code", "bad_code"));
        }
        normalize_posix_path(&self.path)?;
        if self.recommended.is_none() && self.action.is_none() {
            return Err(ContractError::identity(
                "finding.guidance",
                "missing_guidance",
            ));
        }
        if self.reason.trim().is_empty() {
            return Err(ContractError::identity("finding.reason", "empty_reason"));
        }
        Ok(())
    }
}

/// Stable codes: lowercase start, then lowercase/digits/underscore.
fn is_stable_code(code: &str) -> bool {
    let mut bytes = code.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
