// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::canonical::{Digest, canonical_json_bytes};
use crate::errors::ContractError;
use crate::workflow::cache_identity::{check_schema, parse_strict_record, require_nonempty};

use super::validation::require_git_sha;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobBindingV1 {
    pub schema: u32,
    pub repository_id: u64,
    pub repository: String,
    pub run_id: u64,
    pub run_attempt: u64,
    pub job_id: u64,
    pub job_key: String,
    pub event: String,
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub workflow_sha: String,
    pub source_sha: String,
    pub plan_digest: Digest,
}

impl JobBindingV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema, "job.schema")?;
        // `repository_id == 0` means unresolvable in tokenless emit: the
        // group-ledger producer runs without a GitHub token, so the numeric
        // repository and job ids cannot be looked up. Zero is honest here,
        // not a wildcard: ledger identity binds on the repository slug,
        // `run_id`, `run_attempt`, and `job_key`, and no verified consumer
        // reads the numeric ids.
        if self.run_id == 0 {
            return Err(ContractError::identity("job.run_id", "must_be_positive"));
        }
        if self.run_attempt == 0 {
            return Err(ContractError::identity(
                "job.run_attempt",
                "must_be_positive",
            ));
        }
        // `job_id == 0` carries the same tokenless-emit meaning as
        // `repository_id == 0` above: the numeric job id is unresolvable
        // without a token, and identity binds on `job_key` instead.
        // No verified consumer reads the numeric job id.
        require_nonempty("job.repository", &self.repository)?;
        require_nonempty("job.job_key", &self.job_key)?;
        require_nonempty("job.event", &self.event)?;
        require_nonempty("job.ref", &self.ref_name)?;
        require_git_sha("job.workflow_sha", &self.workflow_sha)?;
        require_git_sha("job.source_sha", &self.source_sha)?;
        Ok(())
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
