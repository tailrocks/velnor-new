// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// C16: the full admission inequality admits an exact fit against the
/// operating ceiling and blocks a one-byte-over candidate; the stored sum and
/// the qualified outer envelope are both counted, and an observed maximum is
/// never accepted as proof of the repack bound.
#[expect(
    clippy::too_many_lines,
    reason = "admission case needs fit, overflow, save, and repack"
)]
pub(crate) fn check_admission_exact_fit_and_overflow() -> Result<(), Box<dyn Error>> {
    if HARD_CEILING_BYTES != 10_000_000_000 {
        return Err(invalid("hard ceiling drifted from its policy value"));
    }
    if OPERATING_CEILING_BYTES != 8_000_000_000 {
        return Err(invalid("operating ceiling drifted from its policy value"));
    }
    if MAX_GENERATION_SLOTS != 2 {
        return Err(invalid(
            "generation slot bound drifted from its policy value",
        ));
    }
    let bytes = failure_frozen_bytes();
    let inner = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let allowance = 100_000_000_u64;
    let measured_outer = 500_000_000_u64;
    let bound = measured_outer + allowance;
    let inventory_sum = OPERATING_CEILING_BYTES - bound;
    let generation = failure_generation(b"gc1-qualification-failure-budget")?;
    let inventory = build_inventory(
        1,
        "2026-10-26T00:00:00Z",
        vec![EntryTupleV1 {
            repository_id: 1,
            cache_id: 1610,
            ref_name: "refs/heads/main".to_owned(),
            key: generation.cache_key(),
            version: "v1".to_owned(),
            size_in_bytes: inventory_sum,
            created_at: "2026-10-26T00:00:00Z".to_owned(),
            last_accessed_at: "2026-10-26T00:00:00Z".to_owned(),
        }],
        1,
    )
    .map_err(|error| invalid(&error.to_string()))?;
    if inventory.sum_stored_bytes.value != Some(inventory_sum) {
        return Err(invalid("admission lost its stored-byte count"));
    }
    let job = failure_job("push", "refs/heads/main", 161, 1, 1611, "writer")?;
    let fit = decide_admission(&failure_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        inventory_sum,
        0,
        0,
        inner,
        failure_envelope(&bytes, measured_outer, allowance)?,
        1,
    ))
    .map_err(|error| invalid(&error.to_string()))?;
    if fit.decision != AdmissionDecision::Admitted {
        return Err(invalid("exact fit blocked admission"));
    }
    if fit.candidate_upper_bound_bytes.value != Some(bound) {
        return Err(invalid("admission lost its qualified upper bound"));
    }
    if fit.qualified_repack_allowance_bytes != allowance {
        return Err(invalid("admission lost its repack allowance"));
    }
    let over = decide_admission(&failure_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        inventory_sum + 1,
        0,
        0,
        inner,
        failure_envelope(&bytes, measured_outer, allowance)?,
        1,
    ))
    .map_err(|error| invalid(&error.to_string()))?;
    if over.decision != AdmissionDecision::Blocked {
        return Err(invalid("one-byte-over candidate admitted"));
    }
    if !over
        .blocked_reasons
        .contains(&BLOCKED_INSUFFICIENT_HEADROOM.to_owned())
    {
        return Err(invalid("overflow missed its headroom reason"));
    }
    let oversized_save = failure_save(
        true,
        None,
        true,
        false,
        Some(OPERATING_CEILING_BYTES + 1),
        OPERATING_CEILING_BYTES,
        SaveHalt::Running,
    );
    if decide_save(&oversized_save)? != SaveOutcome::Oversized {
        return Err(invalid("oversized publication escaped its bound"));
    }
    let exact_save = failure_save(
        true,
        None,
        true,
        false,
        Some(OPERATING_CEILING_BYTES),
        OPERATING_CEILING_BYTES,
        SaveHalt::Running,
    );
    if decide_save(&exact_save)? != SaveOutcome::Attempted {
        return Err(invalid("exact-fit save blocked admission"));
    }
    let mut repack_guess = measure_envelope(
        &velnor_actions_orchestrator::FrozenInnerPayload {
            archive_sha256: sha256_hex(&bytes),
            archive_bytes: inner,
        },
        &PreflightMeasurement {
            inner: velnor_actions_orchestrator::FrozenInnerPayload {
                archive_sha256: sha256_hex(&bytes),
                archive_bytes: inner,
            },
            measured_outer_bytes: measured_outer,
            outer_sha256: sha256_hex(b"gc1-qualification-failure-outer"),
        },
        allowance,
        &OuterEnvelopeIdentity {
            tool_identity: "gc1-qualification-failure-packer".to_owned(),
            transport_member: OUTER_MEMBER_NAME.to_owned(),
        },
        true,
    )
    .map_err(|error| invalid(&error.to_string()))?;
    repack_guess.candidate_upper_bound_bytes = bound + 1;
    must_fail(
        &repack_guess.validate(),
        "observed maximum proved the repack bound",
    )?;
    Ok(())
}

/// C16: uncertain inventory, pending writes, foreign exposure, runtime
/// envelope, and local disk each block publication instead of admitting an
/// unbounded write.
#[expect(
    clippy::too_many_lines,
    reason = "uncertainty matrix needs every blocked charge"
)]
pub(crate) fn check_admission_blocks_uncertain() -> Result<(), Box<dyn Error>> {
    let bytes = failure_frozen_bytes();
    let inner = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let generation = failure_generation(b"gc1-qualification-failure-budget")?;
    let job = failure_job("push", "refs/heads/main", 161, 1, 1612, "writer")?;
    let envelope = failure_envelope(&bytes, 500_000_000, 100_000_000)?;
    let mut uncertain = failure_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        envelope,
        1,
    );
    uncertain.inventory_bytes = unknown_bytes("gc1-qualification-failure-inventory")
        .map_err(|error| invalid(&error.to_string()))?;
    let blocked_inventory =
        decide_admission(&uncertain).map_err(|error| invalid(&error.to_string()))?;
    if blocked_inventory.decision != AdmissionDecision::Blocked {
        return Err(invalid("unknown inventory admitted"));
    }
    if !blocked_inventory
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_INVENTORY.to_owned())
    {
        return Err(invalid("unknown inventory missed its reason"));
    }
    let mut pending = failure_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        failure_envelope(&bytes, 500_000_000, 100_000_000)?,
        1,
    );
    pending.pending_owned_bytes = unknown_bytes("gc1-qualification-failure-pending")
        .map_err(|error| invalid(&error.to_string()))?;
    let blocked_pending =
        decide_admission(&pending).map_err(|error| invalid(&error.to_string()))?;
    if !blocked_pending
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_PENDING_WRITES.to_owned())
    {
        return Err(invalid("unknown pending writes missed their reason"));
    }
    let mut foreign = failure_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        failure_envelope(&bytes, 500_000_000, 100_000_000)?,
        1,
    );
    foreign.foreign_inflight_bytes = unknown_bytes("gc1-qualification-failure-foreign")
        .map_err(|error| invalid(&error.to_string()))?;
    let blocked_foreign =
        decide_admission(&foreign).map_err(|error| invalid(&error.to_string()))?;
    if !blocked_foreign
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_FOREIGN_EXPOSURE.to_owned())
    {
        return Err(invalid("unknown foreign exposure missed its reason"));
    }
    let unknown = unknown_envelope(
        UNKNOWN_HOSTED_PACKAGING,
        "gc1-qualification-failure-packaging",
    )
    .map_err(|error| invalid(&error.to_string()))?;
    let unmeasured = failure_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        OuterEnvelope::Unknown(unknown),
        1,
    );
    let blocked_envelope =
        decide_admission(&unmeasured).map_err(|error| invalid(&error.to_string()))?;
    if blocked_envelope.decision != AdmissionDecision::Blocked {
        return Err(invalid("unknown runtime envelope admitted"));
    }
    if !blocked_envelope
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_RUNTIME_ENVELOPE.to_owned())
    {
        return Err(invalid("unknown runtime envelope missed its reason"));
    }
    let mut disk = failure_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        failure_envelope(&bytes, 500_000_000, 100_000_000)?,
        1,
    );
    disk.available_local_disk = unknown_bytes("gc1-qualification-failure-disk")
        .map_err(|error| invalid(&error.to_string()))?;
    let blocked_disk = decide_admission(&disk).map_err(|error| invalid(&error.to_string()))?;
    if !blocked_disk
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_LOCAL_DISK.to_owned())
    {
        return Err(invalid("unknown local disk missed its reason"));
    }
    let mut small_disk = failure_admission(
        job,
        generation,
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        failure_envelope(&bytes, 500_000_000, 100_000_000)?,
        1,
    );
    small_disk.available_local_disk = measured_bytes(10);
    small_disk.peak_local_disk_bound = measured_bytes(100);
    let blocked_small =
        decide_admission(&small_disk).map_err(|error| invalid(&error.to_string()))?;
    if blocked_small.decision != AdmissionDecision::Blocked {
        return Err(invalid("insufficient local disk admitted"));
    }
    if !blocked_small
        .blocked_reasons
        .contains(&BLOCKED_INSUFFICIENT_LOCAL_DISK.to_owned())
    {
        return Err(invalid("insufficient local disk missed its reason"));
    }
    Ok(())
}
