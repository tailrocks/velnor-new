// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn failure_envelope(
    inner_bytes: &[u8],
    measured_outer: u64,
    repack_allowance: u64,
) -> Result<OuterEnvelope, Box<dyn Error>> {
    use velnor_actions_orchestrator::FrozenInnerPayload;
    let inner = FrozenInnerPayload {
        archive_sha256: sha256_hex(inner_bytes),
        archive_bytes: u64::try_from(inner_bytes.len())
            .map_err(|_| invalid("fixture length out of range"))?,
    };
    let preflight = PreflightMeasurement {
        measured_outer_bytes: measured_outer,
        outer_sha256: sha256_hex(b"gc1-qualification-failure-outer"),
    };
    let identity = OuterEnvelopeIdentity {
        tool_identity: "gc1-qualification-failure-packer".to_owned(),
        transport_member: OUTER_MEMBER_NAME.to_owned(),
    };
    let measured = measure_envelope(&inner, &preflight, repack_allowance, &identity, true)
        .map_err(|error| invalid(&error.to_string()))?;
    Ok(OuterEnvelope::Measured(measured))
}

#[expect(
    clippy::too_many_arguments,
    reason = "admission inequality needs every charge"
)]
pub(crate) fn failure_admission(
    job: JobBindingV1,
    generation: GenerationId,
    operating_ceiling: u64,
    inventory_sum: u64,
    pending: u64,
    foreign: u64,
    inner: u64,
    envelope: OuterEnvelope,
    slots: u64,
) -> AdmissionRequest {
    AdmissionRequest {
        job,
        generation,
        active_intent_digest: digest_b3_typed(b"gc1-qualification-failure-intent"),
        inventory_digest: digest_b3_typed(b"gc1-qualification-failure-inventory"),
        operating_ceiling_bytes: operating_ceiling,
        inventory_bytes: measured_bytes(inventory_sum),
        pending_owned_bytes: measured_bytes(pending),
        foreign_inflight_bytes: measured_bytes(foreign),
        candidate_inner_bytes: measured_bytes(inner),
        envelope,
        occupied_generation_slots: slots,
        peak_local_disk_bound: measured_bytes(1_000_000),
        available_local_disk: measured_bytes(100_000_000_000),
        local_disk_reserve: 1_000_000,
        claimed_freed_bytes: 0,
    }
}
