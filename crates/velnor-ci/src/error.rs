// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiError {
    message: String,
    missing: bool,
    transient: bool,
}

impl CiError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            missing: false,
            transient: false,
        }
    }

    pub fn transient(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            missing: false,
            transient: true,
        }
    }

    pub fn missing(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            missing: true,
            transient: false,
        }
    }

    #[must_use]
    pub fn context(self, prefix: &str) -> Self {
        Self {
            message: format!("{prefix}: {}", self.message),
            missing: self.missing,
            transient: self.transient,
        }
    }

    #[must_use]
    pub fn is_transient(&self) -> bool {
        self.transient
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
