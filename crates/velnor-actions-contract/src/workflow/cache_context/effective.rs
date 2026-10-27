// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::canonical::{Digest, canonical_json_bytes};
use crate::errors::ContractError;
use crate::workflow::cache_identity::{parse_strict_record, require_nonempty};

use super::validation::require_git_sha;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiConclusion {
    Success,
    Failure,
    Cancelled,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveJobBindingV1 {
    pub job_key: String,
    pub job_id: u64,
    pub actual_attempt: u64,
    pub source_sha: String,
    pub plan_digest: Digest,
    pub api_conclusion: ApiConclusion,
    pub retained_without_rerun: bool,
    pub ledger_digest: Digest,
}

impl EffectiveJobBindingV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        require_nonempty("effective_job.job_key", &self.job_key)?;
        if self.job_id == 0 {
            return Err(ContractError::identity(
                "effective_job.job_id",
                "must_be_positive",
            ));
        }
        if self.actual_attempt == 0 {
            return Err(ContractError::identity(
                "effective_job.actual_attempt",
                "must_be_positive",
            ));
        }
        require_git_sha("effective_job.source_sha", &self.source_sha)?;
        Ok(())
    }

    #[must_use]
    pub const fn retained_covers(&self, job_id: u64, actual_attempt: u64) -> bool {
        self.retained_without_rerun
            && self.job_id == job_id
            && self.actual_attempt == actual_attempt
    }

    pub fn parse_json(text: &str) -> Result<Self, ContractError> {
        let record: Self = parse_strict_record(text)?;
        record.validate()?;
        Ok(record)
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        canonical_json_bytes(self)
    }
}
