// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ContractError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PremergeEventProofConfig {
    pub repository_id: u64,
    pub owner_id: u64,
    pub environment: String,
    pub reviewer_id: u64,
    pub caller_workflow: String,
}

impl PremergeEventProofConfig {
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let environment = !self.environment.is_empty()
            && self.environment.len() <= 100
            && self
                .environment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
        let workflow = self
            .caller_workflow
            .strip_prefix(".github/workflows/")
            .is_some_and(|name| {
                !name.starts_with('.')
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
                    && name
                        .rsplit_once('.')
                        .is_some_and(|(_, extension)| matches!(extension, "yml" | "yaml"))
            });
        if !environment
            || !workflow
            || self.repository_id == 0
            || self.owner_id == 0
            || self.reviewer_id == 0
            || self.caller_workflow == ".github/workflows/premerge-event-proof.yml"
        {
            return Err(ContractError::config(
                file,
                "premerge_event_proof",
                "invalid_protected_premerge_policy",
            ));
        }
        Ok(())
    }
}
