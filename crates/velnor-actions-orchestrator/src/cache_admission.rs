// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use velnor_actions_contract::canonical::{Digest, canonical_json_bytes, digest_b3_typed};
use velnor_actions_contract::strict_json::parse_strict_json_bytes;
use velnor_actions_contract::workflow::cache_context::{JobBindingV1, MeasuredU64};
use velnor_actions_contract::workflow::cache_identity::GenerationId;

use crate::OrchestratorError;
use crate::cache_archive::cache_archive_envelope::{
    HARD_CEILING_BYTES, MAX_GENERATION_SLOTS, MIN_RESERVE_BYTES, OuterEnvelope,
};
use crate::cache_inventory::{measured_bytes, unknown_bytes};
use crate::internal::internal_contract;

pub const ADMISSION_SCHEMA: u32 = 1;

pub const MAX_ADMISSION_JSON_BYTES: usize = 64 * 1024;

pub const MAX_IDENTITY_BYTES: usize = 512;

pub const BLOCKED_UNKNOWN_INVENTORY: &str = "unknown-inventory";

pub const BLOCKED_UNKNOWN_PENDING_WRITES: &str = "unknown-pending-writes";

pub const BLOCKED_UNKNOWN_FOREIGN_EXPOSURE: &str = "unknown-foreign-exposure";

pub const BLOCKED_UNKNOWN_RUNTIME_ENVELOPE: &str = "unknown-runtime-envelope";

pub const BLOCKED_INSUFFICIENT_HEADROOM: &str = "insufficient-headroom";

pub const BLOCKED_GENERATION_SLOTS_EXHAUSTED: &str = "generation-slots-exhausted";

pub const BLOCKED_UNKNOWN_LOCAL_DISK: &str = "unknown-local-disk";

pub const BLOCKED_INSUFFICIENT_LOCAL_DISK: &str = "insufficient-local-disk";

const BLOCKED_REASONS: [&str; 8] = [
    BLOCKED_UNKNOWN_INVENTORY,
    BLOCKED_UNKNOWN_PENDING_WRITES,
    BLOCKED_UNKNOWN_FOREIGN_EXPOSURE,
    BLOCKED_UNKNOWN_RUNTIME_ENVELOPE,
    BLOCKED_INSUFFICIENT_HEADROOM,
    BLOCKED_GENERATION_SLOTS_EXHAUSTED,
    BLOCKED_UNKNOWN_LOCAL_DISK,
    BLOCKED_INSUFFICIENT_LOCAL_DISK,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionDecision {
    Admitted,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionV1 {
    pub schema: u32,
    pub job: JobBindingV1,
    pub generation: GenerationId,
    pub active_intent_digest: Digest,
    pub inventory_digest: Digest,
    pub hard_ceiling_bytes: u64,
    pub operating_ceiling_bytes: u64,
    pub inventory_bytes: MeasuredU64,
    pub pending_owned_bytes: MeasuredU64,
    pub foreign_inflight_bytes: MeasuredU64,
    pub candidate_inner_bytes: MeasuredU64,
    pub measured_outer_bytes: MeasuredU64,
    pub qualified_repack_allowance_bytes: u64,
    pub outer_envelope_identity: Option<String>,
    pub candidate_upper_bound_bytes: MeasuredU64,
    pub occupied_generation_slots: u64,
    pub maximum_generation_slots: u64,
    pub peak_local_disk_bound: MeasuredU64,
    pub available_local_disk: MeasuredU64,
    pub local_disk_reserve: u64,
    pub decision: AdmissionDecision,
    pub blocked_reasons: Vec<String>,
}

impl AdmissionV1 {
    pub fn validate(&self) -> Result<(), OrchestratorError> {
        self.validate_policy()?;
        for measured in [
            &self.inventory_bytes,
            &self.pending_owned_bytes,
            &self.foreign_inflight_bytes,
            &self.candidate_inner_bytes,
            &self.measured_outer_bytes,
            &self.candidate_upper_bound_bytes,
            &self.peak_local_disk_bound,
            &self.available_local_disk,
        ] {
            measured.validate().map_err(internal_contract)?;
        }
        self.validate_bound()?;
        self.validate_decision()?;
        Ok(())
    }

    fn validate_policy(&self) -> Result<(), OrchestratorError> {
        if self.schema != ADMISSION_SCHEMA {
            return Err(admission_error("admission_schema_unsupported"));
        }
        self.job.validate().map_err(internal_contract)?;
        GenerationId::validate_key(&self.generation.cache_key()).map_err(internal_contract)?;
        if self.hard_ceiling_bytes != HARD_CEILING_BYTES {
            return Err(admission_error("admission_hard_ceiling_drifted"));
        }
        check_operating_ceiling(self.operating_ceiling_bytes)?;
        if self.maximum_generation_slots != MAX_GENERATION_SLOTS {
            return Err(admission_error("admission_generation_slots_drifted"));
        }
        if self.occupied_generation_slots > self.maximum_generation_slots {
            return Err(admission_error("admission_occupied_beyond_maximum"));
        }
        Ok(())
    }

    fn validate_bound(&self) -> Result<(), OrchestratorError> {
        if let Some(identity) = &self.outer_envelope_identity
            && (identity.is_empty()
                || identity.len() > MAX_IDENTITY_BYTES
                || identity.bytes().any(|byte| byte == 0))
        {
            return Err(admission_error("admission_envelope_identity_invalid"));
        }
        match (
            &self.candidate_upper_bound_bytes.value,
            &self.outer_envelope_identity,
        ) {
            (Some(_), Some(_)) | (None, None) => {}
            _ => return Err(admission_error("admission_bound_identity_mismatch")),
        }
        match (
            self.candidate_upper_bound_bytes.value,
            self.measured_outer_bytes.value,
        ) {
            (Some(bound), Some(outer)) => {
                if outer
                    .checked_add(self.qualified_repack_allowance_bytes)
                    .is_none_or(|expected| expected != bound)
                {
                    return Err(admission_error("admission_bound_mismatch"));
                }
            }
            (Some(_), None) => return Err(admission_error("admission_bound_without_outer")),
            (None, _) if self.qualified_repack_allowance_bytes != 0 => {
                return Err(admission_error("admission_allowance_without_bound"));
            }
            (None, _) => {}
        }
        Ok(())
    }

    fn validate_decision(&self) -> Result<(), OrchestratorError> {
        if self
            .blocked_reasons
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
            || self
                .blocked_reasons
                .iter()
                .any(|reason| reason.is_empty() || !BLOCKED_REASONS.contains(&reason.as_str()))
        {
            return Err(admission_error("admission_blocked_reasons_invalid"));
        }
        match self.decision {
            AdmissionDecision::Admitted if !self.blocked_reasons.is_empty() => {
                return Err(admission_error("admission_admitted_with_reasons"));
            }
            AdmissionDecision::Blocked if self.blocked_reasons.is_empty() => {
                return Err(admission_error("admission_blocked_without_reasons"));
            }
            _ => {}
        }
        Ok(())
    }
}

fn check_operating_ceiling(operating: u64) -> Result<(), OrchestratorError> {
    if operating == 0
        || HARD_CEILING_BYTES
            .checked_sub(operating)
            .is_none_or(|reserve| reserve < MIN_RESERVE_BYTES)
    {
        return Err(admission_error("admission_reserve_below_minimum"));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct AdmissionRequest {
    pub job: JobBindingV1,
    pub generation: GenerationId,
    pub active_intent_digest: Digest,
    pub inventory_digest: Digest,
    pub operating_ceiling_bytes: u64,
    pub inventory_bytes: MeasuredU64,
    pub pending_owned_bytes: MeasuredU64,
    pub foreign_inflight_bytes: MeasuredU64,
    pub candidate_inner_bytes: MeasuredU64,
    pub envelope: OuterEnvelope,
    pub occupied_generation_slots: u64,
    pub peak_local_disk_bound: MeasuredU64,
    pub available_local_disk: MeasuredU64,
    pub local_disk_reserve: u64,
    pub claimed_freed_bytes: u64,
}

struct EnvelopeParts {
    measured_outer_bytes: MeasuredU64,
    candidate_upper_bound_bytes: MeasuredU64,
    outer_envelope_identity: Option<String>,
    qualified: bool,
    repack_allowance_bytes: u64,
}

fn envelope_parts(envelope: &OuterEnvelope) -> Result<EnvelopeParts, OrchestratorError> {
    envelope.validate()?;
    match envelope {
        OuterEnvelope::Measured(measured) => Ok(EnvelopeParts {
            measured_outer_bytes: measured_bytes(measured.measured_outer_bytes),
            candidate_upper_bound_bytes: measured_bytes(measured.candidate_upper_bound_bytes),
            outer_envelope_identity: Some(measured.envelope_identity.tool_identity.clone()),
            qualified: measured.hosted_measurement,
            repack_allowance_bytes: measured.repack_allowance_bytes,
        }),
        OuterEnvelope::Unknown(unknown) => {
            let detail = format!("outer-unknown:{}", unknown.reason_code);
            Ok(EnvelopeParts {
                measured_outer_bytes: unknown_bytes(&detail)?,
                candidate_upper_bound_bytes: unknown_bytes(&detail)?,
                outer_envelope_identity: None,
                qualified: false,
                repack_allowance_bytes: 0,
            })
        }
    }
}

fn check_byteceiling_charges(request: &AdmissionRequest) -> Result<(), OrchestratorError> {
    request.job.validate().map_err(internal_contract)?;
    GenerationId::validate_key(&request.generation.cache_key()).map_err(internal_contract)?;
    check_operating_ceiling(request.operating_ceiling_bytes)?;
    if request.occupied_generation_slots > MAX_GENERATION_SLOTS {
        return Err(admission_error("admission_occupied_beyond_maximum"));
    }
    for measured in [
        &request.inventory_bytes,
        &request.pending_owned_bytes,
        &request.foreign_inflight_bytes,
        &request.candidate_inner_bytes,
        &request.peak_local_disk_bound,
        &request.available_local_disk,
    ] {
        measured.validate().map_err(internal_contract)?;
    }
    Ok(())
}

fn collect_blocked(request: &AdmissionRequest, parts: &EnvelopeParts) -> Vec<String> {
    let mut blocked: Vec<String> = Vec::new();
    if request.inventory_bytes.value.is_none() {
        blocked.push(BLOCKED_UNKNOWN_INVENTORY.to_owned());
    }
    if request.pending_owned_bytes.value.is_none() {
        blocked.push(BLOCKED_UNKNOWN_PENDING_WRITES.to_owned());
    }
    if request.foreign_inflight_bytes.value.is_none() {
        blocked.push(BLOCKED_UNKNOWN_FOREIGN_EXPOSURE.to_owned());
    }
    if parts.candidate_upper_bound_bytes.value.is_none() || !parts.qualified {
        blocked.push(BLOCKED_UNKNOWN_RUNTIME_ENVELOPE.to_owned());
    }
    if request.occupied_generation_slots >= MAX_GENERATION_SLOTS {
        blocked.push(BLOCKED_GENERATION_SLOTS_EXHAUSTED.to_owned());
    }
    collect_headroom_block(
        request,
        parts.candidate_upper_bound_bytes.value,
        &mut blocked,
    );
    collect_disk_block(request, &mut blocked);
    blocked.sort();
    blocked.dedup();
    blocked
}

fn collect_headroom_block(
    request: &AdmissionRequest,
    bound: Option<u64>,
    blocked: &mut Vec<String>,
) {
    if let (Some(inventory), Some(pending), Some(foreign), Some(bound)) = (
        request.inventory_bytes.value,
        request.pending_owned_bytes.value,
        request.foreign_inflight_bytes.value,
        bound,
    ) {
        let fits = inventory
            .checked_add(pending)
            .and_then(|total| total.checked_add(foreign))
            .and_then(|total| total.checked_add(bound))
            .is_some_and(|total| total <= request.operating_ceiling_bytes);
        if !fits {
            blocked.push(BLOCKED_INSUFFICIENT_HEADROOM.to_owned());
        }
    }
}

fn collect_disk_block(request: &AdmissionRequest, blocked: &mut Vec<String>) {
    if let (Some(peak), Some(available)) = (
        request.peak_local_disk_bound.value,
        request.available_local_disk.value,
    ) {
        if peak
            .checked_add(request.local_disk_reserve)
            .is_none_or(|required| required > available)
        {
            blocked.push(BLOCKED_INSUFFICIENT_LOCAL_DISK.to_owned());
        }
    } else {
        blocked.push(BLOCKED_UNKNOWN_LOCAL_DISK.to_owned());
    }
}

pub fn decide_admission(request: &AdmissionRequest) -> Result<AdmissionV1, OrchestratorError> {
    if request.claimed_freed_bytes != 0 {
        return Err(admission_error("admission_unconfirmed_deletion_credit"));
    }
    check_byteceiling_charges(request)?;
    let parts = envelope_parts(&request.envelope)?;
    let blocked = collect_blocked(request, &parts);
    let decision = if blocked.is_empty() {
        AdmissionDecision::Admitted
    } else {
        AdmissionDecision::Blocked
    };
    let admission = AdmissionV1 {
        schema: ADMISSION_SCHEMA,
        job: request.job.clone(),
        generation: request.generation.clone(),
        active_intent_digest: request.active_intent_digest.clone(),
        inventory_digest: request.inventory_digest.clone(),
        hard_ceiling_bytes: HARD_CEILING_BYTES,
        operating_ceiling_bytes: request.operating_ceiling_bytes,
        inventory_bytes: request.inventory_bytes.clone(),
        pending_owned_bytes: request.pending_owned_bytes.clone(),
        foreign_inflight_bytes: request.foreign_inflight_bytes.clone(),
        candidate_inner_bytes: request.candidate_inner_bytes.clone(),
        measured_outer_bytes: parts.measured_outer_bytes,
        qualified_repack_allowance_bytes: parts.repack_allowance_bytes,
        outer_envelope_identity: parts.outer_envelope_identity,
        candidate_upper_bound_bytes: parts.candidate_upper_bound_bytes,
        occupied_generation_slots: request.occupied_generation_slots,
        maximum_generation_slots: MAX_GENERATION_SLOTS,
        peak_local_disk_bound: request.peak_local_disk_bound.clone(),
        available_local_disk: request.available_local_disk.clone(),
        local_disk_reserve: request.local_disk_reserve,
        decision,
        blocked_reasons: blocked,
    };
    admission.validate()?;
    Ok(admission)
}

pub fn admission_canonical_bytes(value: &AdmissionV1) -> Result<Vec<u8>, OrchestratorError> {
    value.validate()?;
    canonical_json_bytes(value).map_err(internal_contract)
}

pub fn parse_admission_bytes(bytes: &[u8]) -> Result<AdmissionV1, OrchestratorError> {
    let value =
        parse_strict_json_bytes(bytes, MAX_ADMISSION_JSON_BYTES).map_err(internal_contract)?;
    let admission: AdmissionV1 = serde_json::from_value(value)
        .map_err(|error| admission_error(&format!("admission_json:{error}")))?;
    admission.validate()?;
    Ok(admission)
}

pub fn admission_digest(admission: &AdmissionV1) -> Result<Digest, OrchestratorError> {
    admission.validate()?;
    let bytes = canonical_json_bytes(admission).map_err(internal_contract)?;
    Ok(digest_b3_typed(&bytes))
}

fn admission_error(problem: &str) -> OrchestratorError {
    OrchestratorError::Inventory {
        problem: problem.to_owned(),
    }
}
