// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::workflow_run::{HookSourceError, WorkflowRunError};

use crate::error::HostError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceFault {
    Retryable(&'static str),
    Final(HostError),
}

impl SourceFault {
    pub(crate) const TIMEOUT: Self = Self::Retryable("timeout");

    pub(crate) const fn lookup(error: WorkflowRunError) -> Self {
        match error {
            WorkflowRunError::Uncertain | WorkflowRunError::HttpStatus(408 | 429) => {
                Self::Retryable("source_api_unavailable")
            }
            _ => Self::Final(HostError::Identity),
        }
    }

    pub(crate) const fn compare(error: HookSourceError) -> Self {
        match error {
            HookSourceError::Api(error) => Self::lookup(error),
            _ => Self::Final(HostError::Identity),
        }
    }

    pub(crate) const fn into_host(self) -> HostError {
        match self {
            Self::Retryable(_) => HostError::Identity,
            Self::Final(error) => error,
        }
    }
}

impl From<HostError> for SourceFault {
    fn from(error: HostError) -> Self {
        Self::Final(error)
    }
}
