// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::canonical::{Digest, canonical_json_bytes};
use crate::errors::ContractError;
use crate::workflow::cache_identity::{check_schema, parse_strict_record, require_nonempty};

use super::JobBindingV1;
use super::validation::require_git_sha;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBindingV1 {
    pub schema: u32,
    pub job: JobBindingV1,
    pub admitted_source_sha: String,
    pub admitted_tree_id: String,
    pub source_admission_digest: Digest,
    pub checkout_semantic_digest: Digest,
    pub declared_input_digest: Digest,
    pub git_semantics_digest: Digest,
    pub plan_digest: Digest,
}

impl SourceBindingV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema, "source_binding.schema")?;
        self.job.validate()?;
        require_git_sha(
            "source_binding.admitted_source_sha",
            &self.admitted_source_sha,
        )?;
        require_nonempty("source_binding.admitted_tree_id", &self.admitted_tree_id)?;
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
