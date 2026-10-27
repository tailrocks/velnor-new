use velnor_actions_contract::canonical::{Digest, digest_b3};
use velnor_actions_contract::workflow::cache_context::JobBindingV1;
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};

use super::*;
use crate::cache_archive::cache_archive_envelope::{
    OUTER_MEMBER_NAME, OPERATING_CEILING_BYTES, FrozenInnerPayload, OuterEnvelopeIdentity,
    PreflightMeasurement, measure_envelope,
};
use crate::cache_inventory::measured_bytes;
use crate::source_prep::cache_source_projection::sha256_hex;

const SMALL_INNER: u64 = 1024;
const LARGE_CANDIDATE: u64 = 6_000_000_000;

fn test_job() -> JobBindingV1 {
    JobBindingV1 {
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
        plan_digest: Digest::parse(&digest_b3(b"admission-inner-identity-plan"))
            .expect("plan digest parses"),
    }
}

fn test_generation() -> GenerationId {
    GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(b"admission-inner-group"))
            .expect("group digest parses"),
        compatibility_digest: Digest::parse(&digest_b3(b"admission-inner-compat"))
            .expect("compat digest parses"),
        coverage_digest: Digest::parse(&digest_b3(b"admission-inner-coverage"))
            .expect("coverage digest parses"),
        repair_revision: 0,
    }
}

fn small_payload_envelope() -> OuterEnvelope {
    let inner = FrozenInnerPayload {
        archive_sha256: sha256_hex(b"admission-inner-small-payload"),
        archive_bytes: SMALL_INNER,
    };
    let measured = measure_envelope(
        &inner,
        &PreflightMeasurement {
            inner: inner.clone(),
            measured_outer_bytes: 2048,
            outer_sha256: sha256_hex(b"admission-inner-small-outer"),
        },
        0,
        &OuterEnvelopeIdentity {
            tool_identity: "admission-inner-identity-packer".to_owned(),
            transport_member: OUTER_MEMBER_NAME.to_owned(),
        },
        true,
    )
    .expect("small payload envelope measures");
    OuterEnvelope::Measured(measured)
}

fn admission_request(candidate_inner: u64, envelope: OuterEnvelope) -> AdmissionRequest {
    AdmissionRequest {
        job: test_job(),
        generation: test_generation(),
        active_intent_digest: Digest::parse(&digest_b3(b"admission-inner-intent"))
            .expect("intent digest parses"),
        inventory_digest: Digest::parse(&digest_b3(b"admission-inner-inventory"))
            .expect("inventory digest parses"),
        operating_ceiling_bytes: OPERATING_CEILING_BYTES,
        inventory_bytes: measured_bytes(0),
        pending_owned_bytes: measured_bytes(0),
        foreign_inflight_bytes: measured_bytes(0),
        candidate_inner_bytes: measured_bytes(candidate_inner),
        envelope,
        occupied_generation_slots: 1,
        peak_local_disk_bound: measured_bytes(1_000_000),
        available_local_disk: measured_bytes(100_000_000_000),
        local_disk_reserve: 1_000_000,
        claimed_freed_bytes: 0,
    }
}

#[test]
fn bound_candidate_admits_when_it_fits() {
    let admission = decide_admission(&admission_request(
        SMALL_INNER,
        small_payload_envelope(),
    ))
    .expect("bound candidate decides");
    assert_eq!(admission.decision, AdmissionDecision::Admitted);
    assert_eq!(admission.candidate_inner_bytes.value, Some(SMALL_INNER));
}

#[test]
fn large_candidate_on_small_payload_bound_blocks() {
    // The 2 KiB outer bound alone would fit the ceiling; the 6 GiB
    // candidate must still block because the envelope describes a
    // different (small) payload.
    let admission = decide_admission(&admission_request(
        LARGE_CANDIDATE,
        small_payload_envelope(),
    ))
    .expect("mismatched candidate decides");
    assert_eq!(admission.decision, AdmissionDecision::Blocked);
    assert!(
        admission
            .blocked_reasons
            .contains(&BLOCKED_UNKNOWN_RUNTIME_ENVELOPE.to_owned()),
        "mismatched inner identity must block on the runtime envelope, got {:?}",
        admission.blocked_reasons
    );
    assert!(
        !admission
            .blocked_reasons
            .contains(&BLOCKED_INSUFFICIENT_HEADROOM.to_owned()),
        "alien outer bound must not feed the headroom calculation, got {:?}",
        admission.blocked_reasons
    );
    assert_eq!(
        admission.candidate_inner_bytes.value,
        Some(LARGE_CANDIDATE),
        "the record preserves the request's inner size, not the envelope's",
    );
}
