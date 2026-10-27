// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use velnor_actions_contract::canonical::{Digest, canonical_json_bytes, digest_b3_typed};
use velnor_actions_contract::strict_json::parse_strict_json_bytes;
use velnor_actions_contract::workflow::cache_context::{JobBindingV1, MeasuredU64};
use velnor_actions_contract::workflow::cache_identity::GenerationId;

use super::cache_recovery::{ObservedEntry, validate_sha256_hex};

pub const PUBLICATION_SCHEMA: u32 = 1;

pub const MAX_PUBLICATION_JSON_BYTES: usize = 64 * 1024;

pub const MAX_PUBLICATION_TEXT_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("cache_publication: {problem}")]
pub struct PublicationError {
    pub problem: String,
}

fn publication_error(problem: &str) -> PublicationError {
    PublicationError {
        problem: problem.to_owned(),
    }
}

fn contract_error(error: &velnor_actions_contract::ContractError) -> PublicationError {
    publication_error(&error.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveOutcome {
    SkippedExisting,
    Denied,
    Oversized,
    Interrupted,
    ServiceFailure,
    Attempted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadbackOutcome {
    Verified,
    Corrupt,
    Unavailable,
    NotAttempted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationOutcome {
    Denied,
    Oversized,
    Interrupted,
    Corrupt,
    Verified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveHalt {
    Running,
    Interrupted,
    ServiceFailed,
}

pub struct SaveRequest {
    pub admission_admitted: bool,
    pub candidate_complete: bool,
    pub exact_usable_exists: bool,
    pub denied_reason: Option<String>,
    pub candidate_bytes: Option<u64>,
    pub ceiling_bytes: u64,
    pub save_halt: SaveHalt,
}

fn validate_save_request(request: &SaveRequest) -> Result<(), PublicationError> {
    if request.ceiling_bytes == 0 {
        return Err(publication_error("publication_ceiling_invalid"));
    }
    if request.denied_reason.as_deref().is_some_and(str::is_empty) {
        return Err(publication_error("publication_denial_reason_invalid"));
    }
    if !request.admission_admitted && request.denied_reason.is_none() {
        return Err(publication_error("publication_denial_without_reason"));
    }
    Ok(())
}

pub fn decide_save(request: &SaveRequest) -> Result<SaveOutcome, PublicationError> {
    validate_save_request(request)?;
    if !request.admission_admitted || request.denied_reason.is_some() {
        return Ok(SaveOutcome::Denied);
    }
    if request.exact_usable_exists {
        return Ok(SaveOutcome::SkippedExisting);
    }
    if !request.candidate_complete {
        return Ok(SaveOutcome::Denied);
    }
    if request
        .candidate_bytes
        .is_some_and(|bytes| bytes > request.ceiling_bytes)
    {
        return Ok(SaveOutcome::Oversized);
    }
    match request.save_halt {
        SaveHalt::Running => Ok(SaveOutcome::Attempted),
        SaveHalt::Interrupted => Ok(SaveOutcome::Interrupted),
        SaveHalt::ServiceFailed => Ok(SaveOutcome::ServiceFailure),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationV1 {
    pub schema: u32,
    pub job: JobBindingV1,
    pub generation: GenerationId,
    pub frozen_archive_digest: String,
    pub manifest_digest: Digest,
    pub frozen_proof_digest: Digest,
    pub admission_digest: Digest,
    pub save_step_outcome: Option<String>,
    pub save_outcome: SaveOutcome,
    pub observed_entry: Option<ObservedEntry>,
    pub readback_outcome: ReadbackOutcome,
    pub readback_archive_digest: Option<String>,
    pub readback_manifest_digest: Option<Digest>,
    pub actual_stored_bytes: MeasuredU64,
    pub active_intent_digest: Digest,
}

fn require_optional_text(field: &str, value: Option<&str>) -> Result<(), PublicationError> {
    if value.is_some_and(|text| {
        text.is_empty()
            || text.len() > MAX_PUBLICATION_TEXT_BYTES
            || text.bytes().any(|byte| byte == 0)
    }) {
        return Err(publication_error(&format!("{field}_invalid")));
    }
    Ok(())
}

fn validate_idle_record(value: &PublicationV1) -> Result<(), PublicationError> {
    if value.readback_outcome != ReadbackOutcome::NotAttempted {
        return Err(publication_error("publication_readback_without_save"));
    }
    if value.observed_entry.is_some() {
        return Err(publication_error("publication_entry_without_save"));
    }
    if value.readback_archive_digest.is_some() || value.readback_manifest_digest.is_some() {
        return Err(publication_error(
            "publication_readback_digest_without_save",
        ));
    }
    if value.actual_stored_bytes.value.is_some() {
        return Err(publication_error("publication_stored_bytes_without_save"));
    }
    Ok(())
}

fn validate_verified_record(value: &PublicationV1) -> Result<(), PublicationError> {
    let entry = value
        .observed_entry
        .as_ref()
        .ok_or_else(|| publication_error("publication_save_without_entry"))?;
    if value.readback_archive_digest.as_deref() != Some(value.frozen_archive_digest.as_str()) {
        return Err(publication_error("publication_verified_archive_mismatch"));
    }
    if value.readback_manifest_digest.as_ref() != Some(&value.manifest_digest) {
        return Err(publication_error("publication_verified_manifest_mismatch"));
    }
    let Some(stored) = value.actual_stored_bytes.value else {
        return Err(publication_error("publication_verified_bytes_unknown"));
    };
    if stored != entry.size_in_bytes {
        return Err(publication_error("publication_verified_bytes_mismatch"));
    }
    Ok(())
}

fn validate_corrupt_record(value: &PublicationV1) -> Result<(), PublicationError> {
    if value.observed_entry.is_none() {
        return Err(publication_error("publication_save_without_entry"));
    }
    if value.readback_archive_digest.is_none() && value.readback_manifest_digest.is_none() {
        return Err(publication_error("publication_corrupt_without_evidence"));
    }
    let archive_matches =
        value.readback_archive_digest.as_deref() == Some(value.frozen_archive_digest.as_str());
    let manifest_matches = value.readback_manifest_digest.as_ref() == Some(&value.manifest_digest);
    if value.readback_archive_digest.is_some()
        && value.readback_manifest_digest.is_some()
        && archive_matches
        && manifest_matches
    {
        return Err(publication_error("publication_corrupt_without_mismatch"));
    }
    Ok(())
}

fn validate_unavailable_record(value: &PublicationV1) -> Result<(), PublicationError> {
    if value.observed_entry.is_none() {
        return Err(publication_error("publication_save_without_entry"));
    }
    if value.readback_archive_digest.is_some() || value.readback_manifest_digest.is_some() {
        return Err(publication_error(
            "publication_readback_digest_without_bytes",
        ));
    }
    Ok(())
}

fn validate_record_binding(value: &PublicationV1) -> Result<(), PublicationError> {
    match value.save_outcome {
        SaveOutcome::Denied
        | SaveOutcome::Oversized
        | SaveOutcome::Interrupted
        | SaveOutcome::ServiceFailure => validate_idle_record(value),
        SaveOutcome::SkippedExisting | SaveOutcome::Attempted => match value.readback_outcome {
            ReadbackOutcome::Verified => validate_verified_record(value),
            ReadbackOutcome::Corrupt => validate_corrupt_record(value),
            ReadbackOutcome::Unavailable => validate_unavailable_record(value),
            ReadbackOutcome::NotAttempted => {
                Err(publication_error("publication_save_without_readback"))
            }
        },
    }
}

impl PublicationV1 {
    pub fn validate(&self) -> Result<(), PublicationError> {
        if self.schema != PUBLICATION_SCHEMA {
            return Err(publication_error("publication_schema_unsupported"));
        }
        self.job
            .validate()
            .map_err(|error| contract_error(&error))?;
        GenerationId::parse_key(&self.generation.cache_key())
            .map_err(|_| publication_error("publication_generation_malformed"))?;
        validate_sha256_hex(&self.frozen_archive_digest)
            .map_err(|error| publication_error(&error.problem))?;
        require_optional_text(
            "publication.save_step_outcome",
            self.save_step_outcome.as_deref(),
        )?;
        if let Some(entry) = &self.observed_entry {
            entry
                .validate()
                .map_err(|error| publication_error(&error.problem))?;
            if entry.key != self.generation.cache_key() {
                return Err(publication_error("publication_entry_key_mismatch"));
            }
            if entry.repository_id != self.job.repository_id {
                return Err(publication_error("publication_entry_repository_mismatch"));
            }
        }
        if let Some(digest) = &self.readback_archive_digest {
            validate_sha256_hex(digest).map_err(|error| publication_error(&error.problem))?;
        }
        self.actual_stored_bytes
            .validate()
            .map_err(|error| contract_error(&error))?;
        validate_record_binding(self)?;
        Ok(())
    }
}

pub fn publication_canonical_bytes(value: &PublicationV1) -> Result<Vec<u8>, PublicationError> {
    value.validate()?;
    canonical_json_bytes(value).map_err(|error| contract_error(&error))
}

pub fn parse_publication_bytes(bytes: &[u8]) -> Result<PublicationV1, PublicationError> {
    let parsed = parse_strict_json_bytes(bytes, MAX_PUBLICATION_JSON_BYTES)
        .map_err(|error| contract_error(&error))?;
    let record: PublicationV1 = serde_json::from_value(parsed)
        .map_err(|error| publication_error(&format!("publication_json:{error}")))?;
    record.validate()?;
    Ok(record)
}

pub fn publication_digest(record: &PublicationV1) -> Result<Digest, PublicationError> {
    record.validate()?;
    let bytes = canonical_json_bytes(record).map_err(|error| contract_error(&error))?;
    Ok(digest_b3_typed(&bytes))
}

pub fn classify_publication(
    record: &PublicationV1,
) -> Result<PublicationOutcome, PublicationError> {
    record.validate()?;
    match record.save_outcome {
        SaveOutcome::Denied => Ok(PublicationOutcome::Denied),
        SaveOutcome::Oversized => Ok(PublicationOutcome::Oversized),
        SaveOutcome::Interrupted | SaveOutcome::ServiceFailure => {
            Ok(PublicationOutcome::Interrupted)
        }
        SaveOutcome::SkippedExisting | SaveOutcome::Attempted => match record.readback_outcome {
            ReadbackOutcome::Verified => Ok(PublicationOutcome::Verified),
            ReadbackOutcome::Corrupt => Ok(PublicationOutcome::Corrupt),
            ReadbackOutcome::Unavailable => Ok(PublicationOutcome::Interrupted),
            ReadbackOutcome::NotAttempted => {
                Err(publication_error("publication_readback_not_attempted"))
            }
        },
    }
}
