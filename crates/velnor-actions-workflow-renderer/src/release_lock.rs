// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::is_clean_text;
use crate::{RenderError, steps::scan_for_private_subcommands};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseConcurrency {
    pub group: String,
    pub cancel_in_progress: bool,
}

const FORBIDDEN_LOCK_TOKENS: &[&str] = &[
    "run_id",
    "run_attempt",
    "run_number",
    "github.sha",
    "github.ref",
    "github.event",
    "inputs.",
    "matrix.",
    "version",
    "strategy",
];

impl ReleaseConcurrency {
    pub fn validate(&self) -> Result<(), RenderError> {
        if !is_clean_text(&self.group, 256) {
            return Err(RenderError::InvalidWorkflow(format!(
                "bad_lock_group:{}",
                self.group
            )));
        }
        for token in FORBIDDEN_LOCK_TOKENS {
            if self.group.contains(token) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "forbidden_lock_token:{token}"
                )));
            }
        }
        scan_for_private_subcommands(&self.group)?;
        if self.cancel_in_progress {
            return Err(RenderError::InvalidWorkflow("publisher_cancel".to_owned()));
        }
        Ok(())
    }
}

pub fn check_lock_anchor(group: &str, repository: &str) -> Result<(), RenderError> {
    if group.contains("github.repository") || group.contains(repository) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(
            "unstable_lock_anchor".to_owned(),
        ))
    }
}
