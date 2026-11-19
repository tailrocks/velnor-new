// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiError {
    message: String,
    missing: bool,
}

impl CiError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            missing: false,
        }
    }

    pub fn missing(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            missing: true,
        }
    }

    #[must_use]
    pub fn context(self, prefix: &str) -> Self {
        Self {
            message: format!("{prefix}: {}", self.message),
            missing: self.missing,
        }
    }

    #[must_use]
    pub fn is_missing(&self) -> bool {
        self.missing
    }
}

impl fmt::Display for CiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CiError {}
