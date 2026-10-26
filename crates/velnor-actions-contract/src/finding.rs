// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::canonical::normalize_posix_path;
use crate::errors::ContractError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub code: String,

    pub path: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,

    pub reason: String,
}

impl Finding {
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
