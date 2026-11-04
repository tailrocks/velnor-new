// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use thiserror::Error;

use crate::errors::ContractError;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HelperError {
    #[error("unsafe_path: {path}: {reason}")]
    UnsafePath { path: String, reason: String },
    #[error("internal: {problem}")]
    Internal { problem: String },
    #[error("io: {path}: {problem}")]
    Io { path: String, problem: String },
}

impl HelperError {
    #[must_use]
    pub fn internal(problem: &str) -> Self {
        Self::Internal {
            problem: problem.to_owned(),
        }
    }

    #[must_use]
    pub fn io(path: impl Into<String>, problem: impl Into<String>) -> Self {
        Self::Io {
            path: path.into(),
            problem: problem.into(),
        }
    }
}

impl From<ContractError> for HelperError {
    fn from(error: ContractError) -> Self {
        Self::Internal {
            problem: error.to_string(),
        }
    }
}
