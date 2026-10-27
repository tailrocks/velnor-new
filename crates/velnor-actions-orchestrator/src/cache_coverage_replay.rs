// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use sha2::{Digest as Sha2Digest, Sha256};
use velnor_actions_contract::canonical::{Digest, canonical_json_bytes, digest_b3_typed};
use velnor_actions_contract::strict_json::parse_strict_json_bytes;
use velnor_actions_contract::workflow::cache_context::JobBindingV1;
use velnor_actions_contract::workflow::cache_identity::GenerationId;

pub const REPLAY_SCHEMA: u32 = 1;

pub const MAX_REPLAY_JSON_BYTES: usize = 64 * 1024;

pub const MAX_REPLAY_TEXT_BYTES: usize = 512;

pub const MAX_REPLAY_RECIPE_IDS: usize = 1024;

pub const MAX_REPLAY_GAPS: usize = 128;

pub const MAX_PAIRED_INPUT_BINDINGS: usize = 16;

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("coverage_replay: {problem}")]
pub struct ReplayError {
    pub problem: String,
}

fn replay_error(problem: &str) -> ReplayError {
    ReplayError {
        problem: problem.to_owned(),
    }
}

fn contract_error(error: &velnor_actions_contract::ContractError) -> ReplayError {
    replay_error(&error.to_string())
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

pub fn validate_sha256_hex(value: &str) -> Result<(), ReplayError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(replay_error("replay_digest_invalid"));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenArchive {
    pub archive_sha256: String,
    pub archive_bytes: u64,
}

impl FrozenArchive {
    pub fn validate(&self) -> Result<(), ReplayError> {
        validate_sha256_hex(&self.archive_sha256)?;
        if self.archive_bytes == 0 {
            return Err(replay_error("replay_frozen_empty"));
        }
        Ok(())
    }
}

pub fn freeze_archive(bytes: &[u8]) -> Result<FrozenArchive, ReplayError> {
    if bytes.is_empty() {
        return Err(replay_error("replay_frozen_empty"));
    }
    let frozen = FrozenArchive {
        archive_sha256: sha256_hex(bytes),
        archive_bytes: u64::try_from(bytes.len())
            .map_err(|_| replay_error("replay_frozen_size_range"))?,
    };
    frozen.validate()?;
    Ok(frozen)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MbxInitialState {
    Empty,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationOutcome {
    Pass,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairedInputBinding {
    pub input_role: String,
    pub archive_digest: Digest,
}

impl PairedInputBinding {
    pub fn validate(&self) -> Result<(), ReplayError> {
        require_text("replay.input_role", &self.input_role)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenProofV1 {
    pub schema: u32,
    pub producer_job: JobBindingV1,
    pub generation: GenerationId,
    pub manifest_digest: Digest,
    pub frozen_archive_digest: String,
    pub frozen_archive_bytes: u64,
    pub frozen_at: String,
    pub paired_input_archive_bindings: Vec<PairedInputBinding>,
    pub restore_context_digest: Digest,
    pub restored_state_index_digest: Digest,
    pub restored_projection_index_digest: Digest,
    pub metadata_roundtrip_passed: bool,
    pub ephemeral_mbx_initial_state: MbxInitialState,
    pub ephemeral_mbx_evidence: String,
    pub replay_recipe_ids: Vec<String>,
    pub replay_command_ledger_digest: Digest,
    pub replay_observation_digest: Digest,
    pub eligible_dependency_ids_digest: Digest,
    pub eligible_dependency_count: u64,
    pub unchanged_dependency_compilations: Option<u64>,
    pub covered_origin_acquisitions: Option<u64>,
    pub observation_gaps: Vec<String>,
    pub required_outputs_present: bool,
    pub archive_digest_after_replay: String,
    pub completed_coverage_digest: Digest,
    pub qualification_outcome: QualificationOutcome,
}

impl FrozenProofV1 {
    pub fn validate(&self) -> Result<(), ReplayError> {
        if self.schema != REPLAY_SCHEMA {
            return Err(replay_error("replay_schema_unsupported"));
        }
        self.producer_job
            .validate()
            .map_err(|error| contract_error(&error))?;
        GenerationId::parse_key(&self.generation.cache_key())
            .map_err(|_| replay_error("replay_generation_malformed"))?;
        validate_sha256_hex(&self.frozen_archive_digest)?;
        if self.frozen_archive_bytes == 0 {
            return Err(replay_error("replay_frozen_empty"));
        }
        require_text("replay.frozen_at", &self.frozen_at)?;
        if self.paired_input_archive_bindings.len() > MAX_PAIRED_INPUT_BINDINGS {
            return Err(replay_error("replay_paired_bindings_bound"));
        }
        for binding in &self.paired_input_archive_bindings {
            binding.validate()?;
        }
        if self
            .paired_input_archive_bindings
            .windows(2)
            .any(|pair| pair[0].input_role >= pair[1].input_role)
        {
            return Err(replay_error("replay_paired_bindings_not_sorted_unique"));
        }
        require_text(
            "replay.ephemeral_mbx_evidence",
            &self.ephemeral_mbx_evidence,
        )?;
        if self.replay_recipe_ids.is_empty() || self.replay_recipe_ids.len() > MAX_REPLAY_RECIPE_IDS
        {
            return Err(replay_error("replay_recipe_ids_bound"));
        }
        for recipe_id in &self.replay_recipe_ids {
            require_text("replay.recipe_id", recipe_id)?;
        }
        if self
            .replay_recipe_ids
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(replay_error("replay_recipe_ids_not_sorted_unique"));
        }
        if self.observation_gaps.len() > MAX_REPLAY_GAPS {
            return Err(replay_error("replay_gaps_bound"));
        }
        for gap in &self.observation_gaps {
            require_text("replay.observation_gap", gap)?;
        }
        if self
            .observation_gaps
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(replay_error("replay_gaps_not_sorted_unique"));
        }
        validate_sha256_hex(&self.archive_digest_after_replay)?;
        Ok(())
    }
}

fn require_text(field: &str, value: &str) -> Result<(), ReplayError> {
    if value.is_empty()
        || value.len() > MAX_REPLAY_TEXT_BYTES
        || value.bytes().any(|byte| byte == 0)
    {
        return Err(replay_error(&format!("{field}_invalid")));
    }
    Ok(())
}

pub fn replay_canonical_bytes(value: &FrozenProofV1) -> Result<Vec<u8>, ReplayError> {
    value.validate()?;
    canonical_json_bytes(value).map_err(|error| contract_error(&error))
}

pub fn parse_replay_bytes(bytes: &[u8]) -> Result<FrozenProofV1, ReplayError> {
    let value = parse_strict_json_bytes(bytes, MAX_REPLAY_JSON_BYTES)
        .map_err(|error| contract_error(&error))?;
    let proof: FrozenProofV1 = serde_json::from_value(value)
        .map_err(|error| replay_error(&format!("replay_json:{error}")))?;
    proof.validate()?;
    Ok(proof)
}

pub fn replay_digest(proof: &FrozenProofV1) -> Result<Digest, ReplayError> {
    proof.validate()?;
    let bytes = canonical_json_bytes(proof).map_err(|error| contract_error(&error))?;
    Ok(digest_b3_typed(&bytes))
}

pub fn verify_replay_frozen(
    proof: &FrozenProofV1,
    frozen: &FrozenArchive,
) -> Result<(), ReplayError> {
    proof.validate()?;
    frozen.validate()?;
    if proof.frozen_archive_digest != frozen.archive_sha256
        || proof.frozen_archive_bytes != frozen.archive_bytes
    {
        return Err(replay_error("replay_frozen_binding_mismatch"));
    }
    if proof.archive_digest_after_replay != frozen.archive_sha256 {
        return Err(replay_error("replay_mutated_archive"));
    }
    if proof.eligible_dependency_count == 0 {
        return Err(replay_error("replay_empty_eligible_coverage"));
    }
    match proof.unchanged_dependency_compilations {
        Some(0) => {}
        Some(_) => return Err(replay_error("replay_unchanged_dependency_compiled")),
        None => return Err(replay_error("replay_unchanged_compilations_unknown")),
    }
    if proof.covered_origin_acquisitions.is_none() {
        return Err(replay_error("replay_origin_observation_incomplete"));
    }
    if !proof.observation_gaps.is_empty() {
        return Err(replay_error("replay_observation_gaps_open"));
    }
    if !proof.metadata_roundtrip_passed {
        return Err(replay_error("replay_metadata_roundtrip_failed"));
    }
    if !proof.required_outputs_present {
        return Err(replay_error("replay_required_outputs_missing"));
    }
    if proof.qualification_outcome != QualificationOutcome::Pass {
        return Err(replay_error("replay_qualification_not_pass"));
    }
    Ok(())
}
