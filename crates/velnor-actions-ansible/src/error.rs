// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::sanitize_error_detail;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, thiserror::Error)]
#[error("{code}")]
pub struct AnsibleError {
    code: String,
}

impl AnsibleError {
    #[must_use]
    pub fn new(name: &str, detail: &str) -> Self {
        Self {
            code: format!("{name}:{}", sanitize_error_detail(detail)),
        }
    }

    #[must_use]
    pub fn bare(name: &str) -> Self {
        Self {
            code: name.to_owned(),
        }
    }

    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }
}
