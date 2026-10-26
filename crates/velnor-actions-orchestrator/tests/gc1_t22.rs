// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

macro_rules! must_fail {
    ($result:expr, $case:expr $(,)?) => {
        if $result.is_ok() {
            return Err(fail($case));
        }
    };
}

use std::error::Error;
use std::io;

use velnor_actions_contract::canonical::{Digest, digest_b3, digest_b3_typed};
use velnor_actions_contract::workflow::cache_context::JobBindingV1;
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};
use velnor_actions_orchestrator::{
    AdmissionRequest, AdmissionV1, BLOCKED_GENERATION_SLOTS_EXHAUSTED,
    BLOCKED_INSUFFICIENT_HEADROOM, BLOCKED_UNKNOWN_FOREIGN_EXPOSURE, BLOCKED_UNKNOWN_INVENTORY,
    BLOCKED_UNKNOWN_LOCAL_DISK, BLOCKED_UNKNOWN_RUNTIME_ENVELOPE, EntryTupleV1, HARD_CEILING_BYTES,
    MAX_GENERATION_SLOTS, MIN_RESERVE_BYTES, OPERATING_CEILING_BYTES, OuterEnvelope,
    UNKNOWN_TOOL_IDENTITY, admission_canonical_bytes, build_inventory, decide_admission,
    freeze_inner_payload, inventory_canonical_bytes, inventory_digest, mark_inventory_unknown,
    measure_envelope, measured_bytes, parse_admission_bytes, parse_inventory_bytes, sha256_hex,
    unknown_bytes, unknown_envelope,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn fixture_job() -> Result<JobBindingV1, Box<dyn Error>> {
    Ok(JobBindingV1 {
        schema: 1,
        repository_id: 1,
        repository: "tailrocks/velnor-new".to_owned(),
        run_id: 7,
        run_attempt: 1,
        job_id: 11,
        job_key: "writer".to_owned(),
        event: "push".to_owned(),
        ref_name: "refs/heads/main".to_owned(),
        workflow_sha: format!("{:040x}", 0xa5a5),
        source_sha: format!("{:040x}", 0x5a5a),
        plan_digest: Digest::parse(&digest_b3(b"gc1-t22-plan"))?,
    })
}

fn fixture_generation() -> Result<GenerationId, Box<dyn Error>> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(b"gc1-t22-group"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"gc1-t22-compat"))?,
        coverage_digest: Digest::parse(&digest_b3(b"gc1-t22-coverage"))?,
        repair_revision: 0,
    })
}

fn entry(cache_id: u64, key: &str, ref_name: &str, bytes: u64) -> EntryTupleV1 {
    EntryTupleV1 {
        repository_id: 1,
        cache_id,
        ref_name: ref_name.to_owned(),
        key: key.to_owned(),
        version: "v1".to_owned(),
        size_in_bytes: bytes,
        created_at: "2026-10-25T00:00:00Z".to_owned(),
        last_accessed_at: "2026-10-25T01:00:00Z".to_owned(),
    }
}

fn hosted_envelope(bound: u64) -> Result<OuterEnvelope, Box<dyn Error>> {
    let inner = freeze_inner_payload(&velnor_actions_orchestrator::PackedArchive {
        archive_sha256: sha256_hex(b"gc1-t22-inner"),
        archive_bytes: 1024,
        member_count: 4,
    })?;
    let measured = measure_envelope(
        &inner,
        &velnor_actions_orchestrator::PreflightMeasurement {
            measured_outer_bytes: bound,
            outer_sha256: sha256_hex(b"gc1-t22-outer"),
        },
        0,
        &velnor_actions_orchestrator::OuterEnvelopeIdentity {
            tool_identity: "tar-0.4.46+flate2-1.1.10".to_owned(),
            transport_member: "payload.tar.gz".to_owned(),
        },
        true,
    )?;
    Ok(OuterEnvelope::Measured(measured))
}

fn admission_request(inventory_sum: u64, bound: u64) -> Result<AdmissionRequest, Box<dyn Error>> {
    let entries = vec![
        entry(3, "velnor-gc1-state-old-baseline", "refs/heads/main", 400),
        entry(
            7,
            "velnor-gc1-state-current",
            "refs/heads/main",
            inventory_sum - 400,
        ),
    ];
    let inventory = build_inventory(1, "2026-10-25T02:00:00Z", entries, 2)?;
    let digest = inventory_digest(&inventory)?;
    Ok(AdmissionRequest {
        job: fixture_job()?,
        generation: fixture_generation()?,
        active_intent_digest: digest_b3_typed(b"gc1-t22-intent"),
        inventory_digest: digest,
        operating_ceiling_bytes: OPERATING_CEILING_BYTES,
        inventory_bytes: measured_bytes(
            inventory
                .sum_stored_bytes
                .value
                .ok_or_else(|| fail("complete inventory lost its measured sum"))?,
        ),
        pending_owned_bytes: measured_bytes(0),
        foreign_inflight_bytes: measured_bytes(0),
        candidate_inner_bytes: measured_bytes(1024),
        envelope: hosted_envelope(bound)?,
        occupied_generation_slots: 1,
        peak_local_disk_bound: measured_bytes(1_000_000),
        available_local_disk: measured_bytes(100_000_000),
        local_disk_reserve: 1_000_000,
        claimed_freed_bytes: 0,
    })
}

fn check_ceiling_policy() -> Result<(), Box<dyn Error>> {
    if HARD_CEILING_BYTES != 10_000_000_000
        || OPERATING_CEILING_BYTES != 8_000_000_000
        || MIN_RESERVE_BYTES != 2_000_000_000
        || HARD_CEILING_BYTES - OPERATING_CEILING_BYTES != MIN_RESERVE_BYTES
    {
        return Err(fail("accounting ceiling drifted"));
    }
    if MAX_GENERATION_SLOTS != 2 {
        return Err(fail("generation slot policy drifted"));
    }
    Ok(())
}

fn check_inventory_accounting() -> Result<(), Box<dyn Error>> {
    let entries = vec![
        entry(11, "velnor-gc1-state-group-a", "refs/heads/main", 1_000),
        entry(12, "velnor-gc1-state-group-a-r1", "refs/heads/main", 2_000),
        entry(13, "velnor-gc1-old-namespace", "refs/heads/main", 500),
        entry(14, "velnor-gc1-state-pr-9", "refs/pull/9/merge", 700),
    ];
    let inventory = build_inventory(1, "2026-10-25T02:00:00Z", entries, 3)?;
    if inventory.sum_stored_bytes.value != Some(4_200) {
        return Err(fail("inventory sum drifted from listed entries"));
    }
    if !inventory.complete || inventory.unknown_reason.is_some() {
        return Err(fail("complete inventory reported unknown"));
    }
    let first_digest = inventory_digest(&inventory)?;
    let rebuilt = build_inventory(
        1,
        "2026-10-25T02:00:00Z",
        vec![
            entry(14, "velnor-gc1-state-pr-9", "refs/pull/9/merge", 700),
            entry(11, "velnor-gc1-state-group-a", "refs/heads/main", 1_000),
            entry(13, "velnor-gc1-old-namespace", "refs/heads/main", 500),
            entry(12, "velnor-gc1-state-group-a-r1", "refs/heads/main", 2_000),
        ],
        3,
    )?;
    if inventory_digest(&rebuilt)? != first_digest {
        return Err(fail("inventory digest drifted across page order"));
    }
    let inventory_bytes = inventory_canonical_bytes(&inventory)?;
    if parse_inventory_bytes(&inventory_bytes)? != inventory {
        return Err(fail("inventory strict round trip drifted"));
    }
    Ok(())
}

fn check_admission_round_trip() -> Result<AdmissionV1, Box<dyn Error>> {
    let request = admission_request(4_200, 1_000_000)?;
    let admitted = decide_admission(&request)?;
    if admitted.decision != velnor_actions_orchestrator::AdmissionDecision::Admitted
        || !admitted.blocked_reasons.is_empty()
    {
        return Err(fail("fitting candidate missed admission"));
    }
    if admitted.hard_ceiling_bytes != HARD_CEILING_BYTES
        || admitted.maximum_generation_slots != MAX_GENERATION_SLOTS
    {
        return Err(fail("admission lost its ceiling policy"));
    }
    let admission_bytes = admission_canonical_bytes(&admitted)?;
    let round_trip = parse_admission_bytes(&admission_bytes)?;
    if round_trip != admitted {
        return Err(fail("admission strict round trip drifted"));
    }
    Ok(admitted)
}

fn check_corrupt_stays_charged() -> Result<(), Box<dyn Error>> {
    let corrupt = build_inventory(
        1,
        "2026-10-25T02:00:00Z",
        vec![
            entry(21, "velnor-gc1-state-good", "refs/heads/main", 1_000),
            entry(22, "velnor-gc1-state-corrupt", "refs/heads/main", 2_500),
        ],
        1,
    )?;
    if corrupt.sum_stored_bytes.value != Some(3_500) {
        return Err(fail("corrupt replacement escaped its byte charge"));
    }
    let mut corrupt_request = admission_request(1_000, 1_000_000)?;
    corrupt_request.inventory_bytes = measured_bytes(
        corrupt
            .sum_stored_bytes
            .value
            .ok_or_else(|| fail("corrupt inventory lost its measured sum"))?,
    );
    corrupt_request.inventory_digest = inventory_digest(&corrupt)?;
    corrupt_request.occupied_generation_slots = 2;
    let blocked = decide_admission(&corrupt_request)?;
    if blocked.decision != velnor_actions_orchestrator::AdmissionDecision::Blocked
        || !blocked
            .blocked_reasons
            .contains(&BLOCKED_GENERATION_SLOTS_EXHAUSTED.to_owned())
    {
        return Err(fail("occupied corrupt slot missed its charge"));
    }
    Ok(())
}

#[path = "gc1_t22_a.rs"]
mod gc1_t22_a;
