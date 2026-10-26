// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::distribution::{valid_app_id_variable, valid_review_check_name, valid_secret_name};
use crate::ContractError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewCompletionProvider {
    #[serde(rename = "codex-v1")]
    CodexV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewCompletionConfig {
    pub provider: ReviewCompletionProvider,

    pub check_name: String,

    pub app_id_variable: String,

    pub app_private_key_secret: String,

    pub reviewer_app_id_variable: String,
}

impl ReviewCompletionConfig {
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let bad = |field, problem| {
            ContractError::config(file, format!("workflow.review_completion.{field}"), problem)
        };
        if !valid_review_check_name(&self.check_name) {
            return Err(bad("check_name", "bad_review_check_name"));
        }
        if !valid_app_id_variable(&self.app_id_variable)
            || !valid_app_id_variable(&self.reviewer_app_id_variable)
        {
            return Err(bad("app_id_variable", "bad_app_id_variable"));
        }
        if !valid_secret_name(&self.app_private_key_secret) {
            return Err(bad("app_private_key_secret", "bad_app_private_key_secret"));
        }
        if self.app_id_variable == self.reviewer_app_id_variable {
            return Err(bad(
                "reviewer_app_id_variable",
                "emitter_must_be_independent",
            ));
        }
        Ok(())
    }
}
