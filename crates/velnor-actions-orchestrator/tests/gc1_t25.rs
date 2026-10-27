// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[expect(
    unreachable_pub,
    reason = "T25 module is private to crate_jobs_obligations; the test exercises it by path include"
)]
#[path = "../src/cache_coverage_replay.rs"]
mod cache_coverage_replay;

use std::error::Error;
use std::io;

use velnor_actions_contract::canonical::{Digest, digest_b3, digest_b3_typed};
use velnor_actions_contract::workflow::cache_context::JobBindingV1;
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};

use self::cache_coverage_replay::{
    FrozenArchive, FrozenProofV1, MbxInitialState, PairedInputBinding, QualificationOutcome,
    freeze_archive, parse_replay_bytes, replay_canonical_bytes, replay_digest, validate_sha256_hex,
    verify_replay_frozen,
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
        plan_digest: Digest::parse(&digest_b3(b"gc1-t25-plan"))?,
    })
}

fn fixture_generation() -> Result<GenerationId, Box<dyn Error>> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(b"gc1-t25-group"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"gc1-t25-compat"))?,
        coverage_digest: Digest::parse(&digest_b3(b"gc1-t25-coverage"))?,
        repair_revision: 0,
    })
}

fn fixture_bytes() -> Vec<u8> {
    let mut bytes = b"gc1-t25-frozen-candidate-marker:".to_vec();
    for round in 0_u32..64 {
        bytes.extend_from_slice(&round.to_le_bytes());
    }
    bytes
}

fn fixture_frozen() -> Result<FrozenArchive, Box<dyn Error>> {
    Ok(freeze_archive(&fixture_bytes())?)
}

fn fixture_proof(frozen: &FrozenArchive) -> Result<FrozenProofV1, Box<dyn Error>> {
    Ok(FrozenProofV1 {
        schema: 1,
        producer_job: fixture_job()?,
        generation: fixture_generation()?,
        manifest_digest: digest_b3_typed(b"gc1-t25-manifest"),
        frozen_archive_digest: frozen.archive_sha256.clone(),
        frozen_archive_bytes: frozen.archive_bytes,
        frozen_at: "2026-10-26T00:00:00Z".to_owned(),
        paired_input_archive_bindings: vec![
            PairedInputBinding {
                input_role: "sources".to_owned(),
                archive_digest: digest_b3_typed(b"gc1-t25-sources"),
            },
            PairedInputBinding {
                input_role: "tools".to_owned(),
                archive_digest: digest_b3_typed(b"gc1-t25-tools"),
            },
        ],
        restore_context_digest: digest_b3_typed(b"gc1-t25-restore-context"),
        restored_state_index_digest: digest_b3_typed(b"gc1-t25-state-index"),
        restored_projection_index_digest: digest_b3_typed(b"gc1-t25-projection-index"),
        metadata_roundtrip_passed: true,
        ephemeral_mbx_initial_state: MbxInitialState::Empty,
        ephemeral_mbx_evidence: "ephemeral MBX store observed empty before replay".to_owned(),
        replay_recipe_ids: vec!["check".to_owned(), "clippy".to_owned(), "test".to_owned()],
        replay_command_ledger_digest: digest_b3_typed(b"gc1-t25-ledger"),
        replay_observation_digest: digest_b3_typed(b"gc1-t25-observation"),
        eligible_dependency_ids_digest: digest_b3_typed(b"gc1-t25-eligible"),
        eligible_dependency_count: 3,
        unchanged_dependency_compilations: Some(0),
        covered_origin_acquisitions: Some(2),
        observation_gaps: Vec::new(),
        required_outputs_present: true,
        archive_digest_after_replay: frozen.archive_sha256.clone(),
        completed_coverage_digest: digest_b3_typed(b"gc1-t25-completed"),
        qualification_outcome: QualificationOutcome::Pass,
    })
}

fn check_freeze_binds_digest() -> Result<(), Box<dyn Error>> {
    let frozen = fixture_frozen()?;
    validate_sha256_hex(&frozen.archive_sha256)?;
    if frozen.archive_bytes != u64::try_from(fixture_bytes().len())? {
        return Err(fail("frozen byte count drifted from candidate bytes"));
    }
    let other = freeze_archive(b"gc1-t25-other-candidate")?;
    if other.archive_sha256 == frozen.archive_sha256 {
        return Err(fail("distinct candidates shared a frozen digest"));
    }
    if freeze_archive(&[]).is_ok() {
        return Err(fail("empty candidate produced a frozen archive"));
    }
    if validate_sha256_hex("not-a-digest").is_ok() || validate_sha256_hex(&"A".repeat(64)).is_ok() {
        return Err(fail("malformed digest passed validation"));
    }
    Ok(())
}

fn check_passing_proof_round_trip() -> Result<(), Box<dyn Error>> {
    let frozen = fixture_frozen()?;
    let proof = fixture_proof(&frozen)?;
    verify_replay_frozen(&proof, &frozen)?;
    let bytes = replay_canonical_bytes(&proof)?;
    if parse_replay_bytes(&bytes)? != proof {
        return Err(fail("proof strict round trip drifted"));
    }
    if replay_digest(&proof)? != replay_digest(&parse_replay_bytes(&bytes)?)? {
        return Err(fail("proof digest drifted across round trip"));
    }
    Ok(())
}

fn check_proof_kept_outside_archive() -> Result<(), Box<dyn Error>> {
    let frozen = fixture_frozen()?;
    let proof = fixture_proof(&frozen)?;
    let bytes = replay_canonical_bytes(&proof)?;
    let marker = b"gc1-t25-frozen-candidate-marker:";
    if bytes.windows(marker.len()).any(|window| window == marker) {
        return Err(fail("proof carried frozen archive bytes"));
    }
    Ok(())
}

fn check_not_applicable_mbx_verifies() -> Result<(), Box<dyn Error>> {
    let frozen = fixture_frozen()?;
    let mut proof = fixture_proof(&frozen)?;
    proof.ephemeral_mbx_initial_state = MbxInitialState::NotApplicable;
    "no ephemeral MBX lane on this runner".clone_into(&mut proof.ephemeral_mbx_evidence);
    verify_replay_frozen(&proof, &frozen)?;
    let bytes = replay_canonical_bytes(&proof)?;
    if parse_replay_bytes(&bytes)? != proof {
        return Err(fail("not-applicable MBX proof drifted across round trip"));
    }
    Ok(())
}

#[test]
fn gc1_t25_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_freeze_binds_digest()?;
    check_passing_proof_round_trip()?;
    check_proof_kept_outside_archive()?;
    check_not_applicable_mbx_verifies()?;
    Ok(())
}

macro_rules! must_fail {
    ($result:expr, $case:expr $(,)?) => {
        if $result.is_ok() {
            return Err(fail($case));
        }
    };
}

fn check_rejects_unbound_or_mutated() -> Result<(), Box<dyn Error>> {
    let frozen = fixture_frozen()?;
    let proof = fixture_proof(&frozen)?;
    let mut wrong_digest = proof.clone();
    wrong_digest.frozen_archive_digest = "0".repeat(64);
    must_fail!(
        verify_replay_frozen(&wrong_digest, &frozen),
        "foreign frozen digest",
    );
    let mut wrong_bytes = proof.clone();
    wrong_bytes.frozen_archive_bytes += 1;
    must_fail!(
        verify_replay_frozen(&wrong_bytes, &frozen),
        "frozen byte count drift",
    );
    let mut mutated = proof.clone();
    mutated.archive_digest_after_replay = "1".repeat(64);
    must_fail!(
        verify_replay_frozen(&mutated, &frozen),
        "replay-mutated archive",
    );
    let other = freeze_archive(b"gc1-t25-other-candidate")?;
    must_fail!(
        verify_replay_frozen(&proof, &other),
        "proof verified against another archive",
    );
    Ok(())
}

fn check_rejects_coverage_gaps() -> Result<(), Box<dyn Error>> {
    let frozen = fixture_frozen()?;
    let proof = fixture_proof(&frozen)?;
    let mut compiled = proof.clone();
    compiled.unchanged_dependency_compilations = Some(2);
    must_fail!(
        verify_replay_frozen(&compiled, &frozen),
        "unchanged dependency compiled",
    );
    let mut unknown_compilations = proof.clone();
    unknown_compilations.unchanged_dependency_compilations = None;
    must_fail!(
        verify_replay_frozen(&unknown_compilations, &frozen),
        "unknown compilations passed as zero",
    );
    let mut empty_coverage = proof.clone();
    empty_coverage.eligible_dependency_count = 0;
    must_fail!(
        verify_replay_frozen(&empty_coverage, &frozen),
        "empty eligible coverage",
    );
    let mut unknown_origin = proof.clone();
    unknown_origin.covered_origin_acquisitions = None;
    must_fail!(
        verify_replay_frozen(&unknown_origin, &frozen),
        "incomplete origin observation",
    );
    let mut gaps = proof.clone();
    gaps.observation_gaps = vec!["unobserved-native-coverage".to_owned()];
    must_fail!(
        verify_replay_frozen(&gaps, &frozen),
        "open observation gaps",
    );
    let mut missing_outputs = proof.clone();
    missing_outputs.required_outputs_present = false;
    must_fail!(
        verify_replay_frozen(&missing_outputs, &frozen),
        "missing required outputs",
    );
    let mut metadata_failed = proof.clone();
    metadata_failed.metadata_roundtrip_passed = false;
    must_fail!(
        verify_replay_frozen(&metadata_failed, &frozen),
        "failed metadata round trip",
    );
    let mut failed = proof.clone();
    failed.qualification_outcome = QualificationOutcome::Fail;
    must_fail!(
        verify_replay_frozen(&failed, &frozen),
        "failed qualification outcome",
    );
    let mut no_recipes = proof.clone();
    no_recipes.replay_recipe_ids = Vec::new();
    must_fail!(
        verify_replay_frozen(&no_recipes, &frozen),
        "empty replay recipe set",
    );
    let mut no_evidence = proof.clone();
    no_evidence.ephemeral_mbx_evidence = String::new();
    must_fail!(
        verify_replay_frozen(&no_evidence, &frozen),
        "MBX state without evidence",
    );
    Ok(())
}

fn check_rejects_invalid_records() -> Result<(), Box<dyn Error>> {
    let frozen = fixture_frozen()?;
    let proof = fixture_proof(&frozen)?;
    let mut bad_schema = proof.clone();
    bad_schema.schema = 2;
    must_fail!(replay_canonical_bytes(&bad_schema), "replay schema");
    let mut unsorted_recipes = proof.clone();
    unsorted_recipes.replay_recipe_ids = vec!["test".to_owned(), "check".to_owned()];
    must_fail!(
        replay_canonical_bytes(&unsorted_recipes),
        "unsorted replay recipes",
    );
    let mut duplicate_recipes = proof.clone();
    duplicate_recipes.replay_recipe_ids = vec!["check".to_owned(), "check".to_owned()];
    must_fail!(
        replay_canonical_bytes(&duplicate_recipes),
        "duplicate replay recipes",
    );
    let mut unsorted_bindings = proof.clone();
    unsorted_bindings.paired_input_archive_bindings = vec![
        PairedInputBinding {
            input_role: "tools".to_owned(),
            archive_digest: digest_b3_typed(b"gc1-t25-tools"),
        },
        PairedInputBinding {
            input_role: "sources".to_owned(),
            archive_digest: digest_b3_typed(b"gc1-t25-sources"),
        },
    ];
    must_fail!(
        replay_canonical_bytes(&unsorted_bindings),
        "unsorted paired bindings",
    );
    let mut empty_digest = proof.clone();
    empty_digest.frozen_archive_digest = String::new();
    must_fail!(replay_canonical_bytes(&empty_digest), "empty frozen digest");
    let bytes = replay_canonical_bytes(&proof)?;
    must_fail!(
        parse_replay_bytes(&bytes[..bytes.len() / 2]),
        "truncated replay",
    );
    must_fail!(
        parse_replay_bytes(br#"{"schema":1,"schema":1}"#),
        "duplicate key",
    );
    let mut value = serde_json::to_value(&proof)?;
    value
        .as_object_mut()
        .ok_or_else(|| fail("replay not object"))?
        .insert("unexpected".to_owned(), serde_json::Value::from(1));
    must_fail!(
        parse_replay_bytes(&serde_json::to_vec(&value)?),
        "unknown field",
    );
    Ok(())
}

#[test]
fn gc1_t25_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_unbound_or_mutated()?;
    check_rejects_coverage_gaps()?;
    check_rejects_invalid_records()?;
    Ok(())
}
