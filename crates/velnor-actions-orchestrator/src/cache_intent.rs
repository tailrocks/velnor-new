// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use velnor_actions_contract::canonical::{Digest, canonical_json_bytes, digest_b3_typed};
use velnor_actions_contract::strict_json::parse_strict_json_bytes;

pub const ACTIVE_INTENT_SCHEMA: u32 = 1;

pub const MAX_INTENT_JSON_BYTES: usize = 64 * 1024;

pub const MAX_INTENT_TEXT_BYTES: usize = 512;

pub const MAX_ACTIVE_IDS: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("intent: {problem}")]
pub struct IntentError {
    pub problem: String,
}

fn intent_error(problem: &str) -> IntentError {
    IntentError {
        problem: problem.to_owned(),
    }
}

fn contract_error(error: &velnor_actions_contract::ContractError) -> IntentError {
    intent_error(&error.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveIntentV1 {
    pub schema: u32,
    pub repository_id: u64,
    pub default_ref: String,
    pub observed_head_sha: String,
    pub configuration_digest: Digest,
    pub active_group_ids: Vec<String>,
    pub active_coverage_ids: Vec<String>,
    pub derived_at: String,
    pub derivation_evidence_digest: Digest,
}

impl ActiveIntentV1 {
    pub fn validate(&self) -> Result<(), IntentError> {
        if self.schema != ACTIVE_INTENT_SCHEMA {
            return Err(intent_error("intent_schema_unsupported"));
        }
        if self.repository_id == 0 {
            return Err(intent_error("intent_repository_id_not_positive"));
        }
        require_text("intent.default_ref", &self.default_ref)?;
        require_sha("intent.observed_head_sha", &self.observed_head_sha)?;
        require_text("intent.derived_at", &self.derived_at)?;
        require_sorted_unique("intent.active_group_ids", &self.active_group_ids)?;
        require_sorted_unique("intent.active_coverage_ids", &self.active_coverage_ids)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentObservation {
    pub observed_head_sha: String,
    pub configuration_digest: Digest,
    pub active_group_ids: Vec<String>,
    pub active_coverage_ids: Vec<String>,
}

impl IntentObservation {
    pub fn validate(&self) -> Result<(), IntentError> {
        require_sha("observation.observed_head_sha", &self.observed_head_sha)?;
        require_sorted_unique("observation.active_group_ids", &self.active_group_ids)?;
        require_sorted_unique("observation.active_coverage_ids", &self.active_coverage_ids)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentRecheck {
    CoverageUnchanged,
    CoverageChanged,
    MutationBlocked,
}

#[must_use]
pub fn intent_authorizes_mutation(recheck: IntentRecheck) -> bool {
    recheck == IntentRecheck::CoverageUnchanged
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentDraft {
    pub repository_id: u64,
    pub default_ref: String,
    pub observed_head_sha: String,
    pub configuration_digest: Digest,
    pub active_group_ids: Vec<String>,
    pub active_coverage_ids: Vec<String>,
    pub derived_at: String,
    pub derivation_evidence_digest: Digest,
}

pub fn derive_intent(mut draft: IntentDraft) -> Result<ActiveIntentV1, IntentError> {
    draft.active_group_ids.sort();
    draft.active_group_ids.dedup();
    draft.active_coverage_ids.sort();
    draft.active_coverage_ids.dedup();
    let intent = ActiveIntentV1 {
        schema: ACTIVE_INTENT_SCHEMA,
        repository_id: draft.repository_id,
        default_ref: draft.default_ref,
        observed_head_sha: draft.observed_head_sha,
        configuration_digest: draft.configuration_digest,
        active_group_ids: draft.active_group_ids,
        active_coverage_ids: draft.active_coverage_ids,
        derived_at: draft.derived_at,
        derivation_evidence_digest: draft.derivation_evidence_digest,
    };
    intent.validate()?;
    Ok(intent)
}

pub fn recheck_intent(
    previous: &ActiveIntentV1,
    current: Option<&IntentObservation>,
) -> Result<IntentRecheck, IntentError> {
    previous.validate()?;
    let Some(observation) = current else {
        return Ok(IntentRecheck::MutationBlocked);
    };
    observation.validate()?;
    if observation.configuration_digest != previous.configuration_digest {
        return Ok(IntentRecheck::CoverageChanged);
    }
    if observation.active_group_ids != previous.active_group_ids {
        return Ok(IntentRecheck::CoverageChanged);
    }
    if observation.active_coverage_ids != previous.active_coverage_ids {
        return Ok(IntentRecheck::CoverageChanged);
    }
    Ok(IntentRecheck::CoverageUnchanged)
}

pub fn intent_canonical_bytes(value: &ActiveIntentV1) -> Result<Vec<u8>, IntentError> {
    value.validate()?;
    canonical_json_bytes(value).map_err(|error| contract_error(&error))
}

pub fn parse_intent_bytes(bytes: &[u8]) -> Result<ActiveIntentV1, IntentError> {
    let value = parse_strict_json_bytes(bytes, MAX_INTENT_JSON_BYTES)
        .map_err(|error| contract_error(&error))?;
    let intent: ActiveIntentV1 = serde_json::from_value(value)
        .map_err(|error| intent_error(&format!("intent_json:{error}")))?;
    intent.validate()?;
    Ok(intent)
}

pub fn intent_digest(intent: &ActiveIntentV1) -> Result<Digest, IntentError> {
    intent.validate()?;
    let bytes = canonical_json_bytes(intent).map_err(|error| contract_error(&error))?;
    Ok(digest_b3_typed(&bytes))
}

fn require_text(field: &str, value: &str) -> Result<(), IntentError> {
    if value.is_empty()
        || value.len() > MAX_INTENT_TEXT_BYTES
        || value.bytes().any(|byte| byte == 0)
    {
        return Err(intent_error(&format!("{field}_invalid")));
    }
    Ok(())
}

fn require_sha(field: &str, value: &str) -> Result<(), IntentError> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(intent_error(&format!("{field}_malformed")));
    }
    Ok(())
}

fn require_sorted_unique(field: &str, values: &[String]) -> Result<(), IntentError> {
    if values.len() > MAX_ACTIVE_IDS {
        return Err(intent_error(&format!("{field}_bound")));
    }
    for value in values {
        require_text(field, value)?;
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(intent_error(&format!("{field}_not_sorted_unique")));
    }
    Ok(())
}
