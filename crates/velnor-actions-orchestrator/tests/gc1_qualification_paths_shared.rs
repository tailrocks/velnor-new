// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn check_paths_harness(case_id: &str, detail: &str) -> Result<(), Box<dyn Error>> {
    require_known_case_ids(&[case_id.to_owned()])?;
    require_nonempty_workload(&Workload {
        recipe_ids: vec!["check".to_owned(), "clippy".to_owned(), "test".to_owned()],
        eligible_dependency_ids: eligible_ids(),
    })?;
    require_complete_observation(&ObservationClosure {
        complete: true,
        detail: detail.to_owned(),
    })?;
    Ok(())
}

pub(crate) fn paths_job(
    event: &str,
    ref_name: &str,
    run_id: u64,
    run_attempt: u64,
    job_id: u64,
    job_key: &str,
) -> Result<JobBindingV1, Box<dyn Error>> {
    let job = JobBindingV1 {
        schema: 1,
        repository_id: 1,
        repository: "tailrocks/velnor-new".to_owned(),
        run_id,
        run_attempt,
        job_id,
        job_key: job_key.to_owned(),
        event: event.to_owned(),
        ref_name: ref_name.to_owned(),
        workflow_sha: format!("{:040x}", 0xa5a5),
        source_sha: format!("{:040x}", 0x5a5a),
        plan_digest: Digest::parse(&digest_b3(b"gc1-qualification-paths-plan"))?,
    };
    job.validate()?;
    Ok(job)
}

pub(crate) fn paths_generation(
    coverage_seed: &[u8],
    group_seed: &[u8],
) -> Result<GenerationId, Box<dyn Error>> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(group_seed))?,
        compatibility_digest: Digest::parse(&digest_b3(b"gc1-qualification-paths-compat"))?,
        coverage_digest: Digest::parse(&digest_b3(coverage_seed))?,
        repair_revision: 0,
    })
}

pub(crate) fn paths_entry(
    generation: &GenerationId,
    ref_name: &str,
    cache_id: u64,
    provenance: EntryProvenance,
) -> SelectableEntry {
    SelectableEntry {
        cache_id,
        repository_id: 1,
        ref_name: ref_name.to_owned(),
        key: generation.cache_key(),
        version: "v1".to_owned(),
        size_in_bytes: 512,
        created_at: "2026-10-26T00:00:00Z".to_owned(),
        provenance,
    }
}

pub(crate) fn paths_discovery(
    job: JobBindingV1,
    generation: GenerationId,
    allowed_refs: Vec<&str>,
    current_ref: &str,
    entries: Vec<SelectableEntry>,
    inventory_complete: bool,
) -> DiscoveryRequest {
    DiscoveryRequest {
        job,
        generation,
        expected_cache_version: "v1".to_owned(),
        allowed_refs: allowed_refs.into_iter().map(str::to_owned).collect(),
        current_ref: current_ref.to_owned(),
        inventory_digest: digest_b3_typed(b"gc1-qualification-paths-inventory"),
        inventory_complete,
        entries,
        active_coverage_ids: vec!["coverage-a".to_owned()],
    }
}

pub(crate) fn paths_frozen_bytes() -> Vec<u8> {
    let mut bytes = b"gc1-qualification-paths-candidate:".to_vec();
    for round in 0_u32..64 {
        bytes.extend_from_slice(&round.to_le_bytes());
    }
    bytes
}

pub(crate) fn paths_envelope(
    inner_bytes: &[u8],
    measured_outer: u64,
    repack_allowance: u64,
) -> Result<OuterEnvelope, Box<dyn Error>> {
    let inner = FrozenInnerPayload {
        archive_sha256: sha256_hex(inner_bytes),
        archive_bytes: u64::try_from(inner_bytes.len())
            .map_err(|_| invalid("fixture length out of range"))?,
    };
    let preflight = PreflightMeasurement {
        inner: inner.clone(),
        measured_outer_bytes: measured_outer,
        outer_sha256: sha256_hex(b"gc1-qualification-paths-outer"),
    };
    let identity = OuterEnvelopeIdentity {
        tool_identity: "gc1-qualification-paths-packer".to_owned(),
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
pub(crate) fn paths_admission(
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
        active_intent_digest: digest_b3_typed(b"gc1-qualification-paths-intent"),
        inventory_digest: digest_b3_typed(b"gc1-qualification-paths-inventory"),
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

/// C18: a named check lane keeps its variant and job binding through the
/// serialized boundary; an unknown variant is rejected and the hosted and
/// scale-set variants stay distinct.
pub(crate) fn check_named_lane_identity() -> Result<(), Box<dyn Error>> {
    let lane = NamedCheckLane {
        variant: Some(NamedCheckLaneVariant::Hosted),
        job_id: "check".to_owned(),
    };
    if lane.job_id.is_empty() {
        return Err(invalid("named lane lost its job binding"));
    }
    let bytes = serde_json::to_vec(&lane).map_err(|error| invalid(&error.to_string()))?;
    let parsed: NamedCheckLane =
        serde_json::from_slice(&bytes).map_err(|error| invalid(&error.to_string()))?;
    if parsed != lane {
        return Err(invalid("named lane lost its round trip"));
    }
    if parsed.variant != Some(NamedCheckLaneVariant::Hosted) {
        return Err(invalid("named lane changed its variant"));
    }
    let unpinned = NamedCheckLane {
        variant: None,
        job_id: "check".to_owned(),
    };
    if unpinned.variant == lane.variant {
        return Err(invalid("unpinned lane merged with the hosted lane"));
    }
    let scaled = NamedCheckLane {
        variant: Some(NamedCheckLaneVariant::ScaleSet),
        job_id: "check".to_owned(),
    };
    if scaled == lane {
        return Err(invalid("scale-set lane merged with the hosted lane"));
    }
    must_fail(
        &serde_json::from_str::<NamedCheckLane>(r#"{"variant":"gpu","job_id":"check"}"#)
            .map(|_| ()),
        "unknown lane variant admitted",
    )?;
    must_fail(
        &serde_json::from_str::<NamedCheckLane>(r#"{"variant":"hosted"}"#).map(|_| ()),
        "lane without a job binding admitted",
    )?;
    Ok(())
}
