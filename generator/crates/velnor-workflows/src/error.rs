// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowsError(pub(crate) String);

impl fmt::Display for WorkflowsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, ".velnor/workflows.toml: {}", self.0)
    }
}

impl Error for WorkflowsError {}

pub(crate) fn refuse<T>(message: impl Into<String>) -> Result<T, WorkflowsError> {
    Err(WorkflowsError(message.into()))
}

impl WorkflowsError {
    #[must_use]
    pub fn invalid(message: String) -> Self {
        Self(message)
    }
}
