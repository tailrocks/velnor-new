// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Concurrency {
    pub group: String,

    pub cancel_in_progress: String,
}

pub const WRITER_QUEUE_CANCEL_IN_PROGRESS: &str = "false";

const WRITER_QUEUE_NESTING_TOKENS: [&str; 4] = ["run_id", "run_attempt", "matrix.", "needs."];

impl Concurrency {
    #[must_use]
    pub fn writer_queue(group: impl Into<String>) -> Self {
        Self {
            group: group.into(),
            cancel_in_progress: WRITER_QUEUE_CANCEL_IN_PROGRESS.to_owned(),
        }
    }

    #[must_use]
    pub fn is_writer_queue(&self) -> bool {
        !self.group.trim().is_empty() && self.cancel_in_progress == WRITER_QUEUE_CANCEL_IN_PROGRESS
    }

    pub fn validate_writer_queue(&self) -> Result<(), ContractError> {
        if self.group.trim().is_empty() {
            return Err(ContractError::identity(
                "workflow.concurrency",
                "empty_group",
            ));
        }
        if self.cancel_in_progress != WRITER_QUEUE_CANCEL_IN_PROGRESS {
            return Err(ContractError::identity(
                "workflow.concurrency",
                "writer_queue_cancel",
            ));
        }
        for token in WRITER_QUEUE_NESTING_TOKENS {
            if self.group.contains(token) {
                return Err(ContractError::identity(
                    "workflow.concurrency",
                    format!("writer_queue_nested_lock:{token}"),
                ));
            }
        }
        Ok(())
    }
}
