// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use tar::{Builder, EntryType, Header};
use velnor_actions_contract::canonical_json_bytes;
use velnor_actions_contract::strict_json::parse_strict_json_bytes;

use super::PackedArchive;
use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::source_prep::cache_source_projection::{sha256_hex, validate_sha256};

pub const ENVELOPE_SCHEMA: u32 = 1;

pub const HARD_CEILING_BYTES: u64 = 10_000_000_000;

pub const OPERATING_CEILING_BYTES: u64 = 8_000_000_000;

pub const MIN_RESERVE_BYTES: u64 = HARD_CEILING_BYTES - OPERATING_CEILING_BYTES;

pub const MAX_GENERATION_SLOTS: u64 = 2;

pub const OUTER_MEMBER_NAME: &str = "payload.tar.gz";

pub const MAX_OUTER_BYTES: u64 = 4 * 1024 * 1024 * 1024;

pub const MAX_ENVELOPE_JSON_BYTES: usize = 64 * 1024;

pub const UNKNOWN_HOSTED_PACKAGING: &str = "unknown-hosted-packaging";

pub const UNKNOWN_TOOL_IDENTITY: &str = "unknown-tool-identity";

pub const UNKNOWN_REPACK_BASIS: &str = "unknown-repack-basis";

pub const UNKNOWN_RUNNER_LAYOUT: &str = "unknown-runner-layout";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenInnerPayload {
    pub archive_sha256: String,
    pub archive_bytes: u64,
}

impl FrozenInnerPayload {
    pub fn validate(&self) -> Result<(), OrchestratorError> {
        validate_sha256(&self.archive_sha256)?;
        if self.archive_bytes == 0 {
            return Err(internal("envelope_inner_empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OuterEnvelopeIdentity {
    pub tool_identity: String,
    pub transport_member: String,
}

impl OuterEnvelopeIdentity {
    pub fn validate(&self) -> Result<(), OrchestratorError> {
        if self.tool_identity.is_empty() || self.tool_identity.bytes().any(|byte| byte == 0) {
            return Err(internal("envelope_tool_identity"));
        }
        if self.transport_member != OUTER_MEMBER_NAME {
            return Err(internal("envelope_transport_member"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeasuredEnvelope {
    pub schema: u32,
    pub inner: FrozenInnerPayload,
    pub measured_outer_bytes: u64,
    pub outer_sha256: String,
    pub repack_allowance_bytes: u64,
    pub candidate_upper_bound_bytes: u64,
    pub envelope_identity: OuterEnvelopeIdentity,
    pub hosted_measurement: bool,
}

impl MeasuredEnvelope {
    pub fn validate(&self) -> Result<(), OrchestratorError> {
        if self.schema != ENVELOPE_SCHEMA {
            return Err(internal("envelope_schema_unsupported"));
        }
        self.inner.validate()?;
        self.envelope_identity.validate()?;
        validate_sha256(&self.outer_sha256)?;
        if self.measured_outer_bytes == 0 {
            return Err(internal("envelope_outer_empty"));
        }
        let expected = self
            .measured_outer_bytes
            .checked_add(self.repack_allowance_bytes)
            .ok_or_else(|| internal("envelope_bound_overflow"))?;
        if expected != self.candidate_upper_bound_bytes {
            return Err(internal("envelope_bound_mismatch"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnknownEnvelope {
    pub schema: u32,
    pub reason_code: String,
    pub detail: String,
}

impl UnknownEnvelope {
    pub fn validate(&self) -> Result<(), OrchestratorError> {
        if self.schema != ENVELOPE_SCHEMA {
            return Err(internal("envelope_schema_unsupported"));
        }
        match self.reason_code.as_str() {
            x if x == UNKNOWN_HOSTED_PACKAGING
                || x == UNKNOWN_TOOL_IDENTITY
                || x == UNKNOWN_REPACK_BASIS
                || x == UNKNOWN_RUNNER_LAYOUT =>
            {
                if self.detail.is_empty() || self.detail.bytes().any(|byte| byte == 0) {
                    return Err(internal("envelope_unknown_detail"));
                }
                Ok(())
            }
            _ => Err(internal("envelope_unknown_code")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum OuterEnvelope {
    Measured(MeasuredEnvelope),
    Unknown(UnknownEnvelope),
}

impl OuterEnvelope {
    pub fn validate(&self) -> Result<(), OrchestratorError> {
        match self {
            OuterEnvelope::Measured(measured) => measured.validate(),
            OuterEnvelope::Unknown(unknown) => unknown.validate(),
        }
    }

    #[must_use]
    pub fn is_measured(&self) -> bool {
        matches!(self, OuterEnvelope::Measured(_))
    }

    #[must_use]
    pub fn candidate_upper_bound_bytes(&self) -> Option<u64> {
        match self {
            OuterEnvelope::Measured(measured) => Some(measured.candidate_upper_bound_bytes),
            OuterEnvelope::Unknown(_) => None,
        }
    }

    #[must_use]
    pub fn qualifies_for_publication(&self) -> bool {
        match self {
            OuterEnvelope::Measured(measured) => measured.hosted_measurement,
            OuterEnvelope::Unknown(_) => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightMeasurement {
    pub inner: FrozenInnerPayload,
    pub measured_outer_bytes: u64,
    pub outer_sha256: String,
}

pub fn freeze_inner_payload(
    packed: &PackedArchive,
) -> Result<FrozenInnerPayload, OrchestratorError> {
    let frozen = FrozenInnerPayload {
        archive_sha256: packed.archive_sha256.clone(),
        archive_bytes: packed.archive_bytes,
    };
    frozen.validate()?;
    Ok(frozen)
}

pub fn run_preflight_packaging(
    inner_archive: &Path,
    preflight_out: &Path,
) -> Result<PreflightMeasurement, OrchestratorError> {
    let metadata = fs::symlink_metadata(inner_archive).map_err(|error| {
        OrchestratorError::io(inner_archive.display().to_string(), error.to_string())
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(internal("envelope_inner_not_regular"));
    }
    if metadata.len() == 0 {
        return Err(internal("envelope_inner_empty"));
    }
    let parent = preflight_out.parent().unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(internal("envelope_preflight_parent_missing"));
    }
    if fs::symlink_metadata(preflight_out).is_ok() {
        return Err(OrchestratorError::OverwriteRefused {
            path: preflight_out.display().to_string(),
        });
    }
    let bytes = fs::read(inner_archive).map_err(|error| {
        OrchestratorError::io(inner_archive.display().to_string(), error.to_string())
    })?;
    let file = File::create_new(preflight_out).map_err(|error| {
        OrchestratorError::io(preflight_out.display().to_string(), error.to_string())
    })?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = Builder::new(encoder);
    let size = u64::try_from(bytes.len()).map_err(|_| internal("envelope_size_range"))?;
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Regular);
    header.set_size(size);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    builder
        .append_data(&mut header, OUTER_MEMBER_NAME, bytes.as_slice())
        .map_err(|error| internal(&format!("envelope_preflight_write:{error}")))?;
    let encoder = builder
        .into_inner()
        .map_err(|error| internal(&format!("envelope_preflight_finish:{error}")))?;
    let mut file = encoder
        .finish()
        .map_err(|error| internal(&format!("envelope_preflight_finish:{error}")))?;
    file.flush().map_err(|error| {
        OrchestratorError::io(preflight_out.display().to_string(), error.to_string())
    })?;
    drop(file);
    let outer = fs::read(preflight_out).map_err(|error| {
        OrchestratorError::io(preflight_out.display().to_string(), error.to_string())
    })?;
    let measured_outer_bytes =
        u64::try_from(outer.len()).map_err(|_| internal("envelope_size_range"))?;
    if measured_outer_bytes == 0 || measured_outer_bytes > MAX_OUTER_BYTES {
        return Err(internal("envelope_outer_limit"));
    }
    let inner = FrozenInnerPayload {
        archive_sha256: sha256_hex(&bytes),
        archive_bytes: size,
    };
    inner.validate()?;
    Ok(PreflightMeasurement {
        inner,
        measured_outer_bytes,
        outer_sha256: sha256_hex(&outer),
    })
}

pub fn measure_envelope(
    inner: &FrozenInnerPayload,
    preflight: &PreflightMeasurement,
    repack_allowance_bytes: u64,
    envelope_identity: &OuterEnvelopeIdentity,
    hosted_measurement: bool,
) -> Result<MeasuredEnvelope, OrchestratorError> {
    inner.validate()?;
    envelope_identity.validate()?;
    if preflight.inner != *inner {
        return Err(internal("envelope_preflight_inner_mismatch"));
    }
    let candidate_upper_bound_bytes = preflight
        .measured_outer_bytes
        .checked_add(repack_allowance_bytes)
        .ok_or_else(|| internal("envelope_bound_overflow"))?;
    let measured = MeasuredEnvelope {
        schema: ENVELOPE_SCHEMA,
        inner: inner.clone(),
        measured_outer_bytes: preflight.measured_outer_bytes,
        outer_sha256: preflight.outer_sha256.clone(),
        repack_allowance_bytes,
        candidate_upper_bound_bytes,
        envelope_identity: envelope_identity.clone(),
        hosted_measurement,
    };
    measured.validate()?;
    Ok(measured)
}

pub fn unknown_envelope(
    reason_code: &str,
    detail: &str,
) -> Result<UnknownEnvelope, OrchestratorError> {
    let unknown = UnknownEnvelope {
        schema: ENVELOPE_SCHEMA,
        reason_code: reason_code.to_owned(),
        detail: detail.to_owned(),
    };
    unknown.validate()?;
    Ok(unknown)
}

pub fn envelope_canonical_bytes(value: &OuterEnvelope) -> Result<Vec<u8>, OrchestratorError> {
    value.validate()?;
    canonical_json_bytes(value).map_err(internal_contract)
}

pub fn parse_envelope_bytes(bytes: &[u8]) -> Result<OuterEnvelope, OrchestratorError> {
    let value =
        parse_strict_json_bytes(bytes, MAX_ENVELOPE_JSON_BYTES).map_err(internal_contract)?;
    let envelope: OuterEnvelope = serde_json::from_value(value)
        .map_err(|error| internal(&format!("envelope_json:{error}")))?;
    envelope.validate()?;
    Ok(envelope)
}
