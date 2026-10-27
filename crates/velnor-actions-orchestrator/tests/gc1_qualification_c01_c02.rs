// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn c01() -> Result<(), Box<dyn Error>> {
    require_nonempty_workload(&workload())?;
    require_complete_observation(&observation("ledger-drained cold observation"))?;
    let report = finish_cold_session()?;
    check_cold_report(&report)?;
    check_rejects_query_control()?;
    Ok(())
}

fn frozen_candidate() -> Result<FrozenArchive, Box<dyn Error>> {
    let mut bytes = b"gc1-qualification-frozen-candidate:".to_vec();
    for round in 0_u32..64 {
        bytes.extend_from_slice(&round.to_le_bytes());
    }
    freeze_archive(&bytes).map_err(|error| invalid(&error.to_string()))
}

fn replay_proof(frozen: &FrozenArchive) -> Result<FrozenProofV1, Box<dyn Error>> {
    let proof = FrozenProofV1 {
        schema: 1,
        producer_job: fixture_job("writer")?,
        generation: generation_for(b"gc1-qualification-coverage")?,
        manifest_digest: digest_b3_typed(b"gc1-qualification-manifest"),
        frozen_archive_digest: frozen.archive_sha256.clone(),
        frozen_archive_bytes: frozen.archive_bytes,
        frozen_at: "2026-10-26T00:00:00Z".to_owned(),
        paired_input_archive_bindings: vec![
            PairedInputBinding {
                input_role: "sources".to_owned(),
                archive_digest: digest_b3_typed(b"gc1-qualification-sources"),
            },
            PairedInputBinding {
                input_role: "tools".to_owned(),
                archive_digest: digest_b3_typed(b"gc1-qualification-tools"),
            },
        ],
        restore_context_digest: digest_b3_typed(b"gc1-qualification-restore-context"),
        restored_state_index_digest: digest_b3_typed(b"gc1-qualification-state-index"),
        restored_projection_index_digest: digest_b3_typed(b"gc1-qualification-projection-index"),
        metadata_roundtrip_passed: true,
        ephemeral_mbx_initial_state: MbxInitialState::Empty,
        ephemeral_mbx_evidence: "ephemeral MBX store observed empty before replay".to_owned(),
        replay_recipe_ids: vec!["check".to_owned(), "clippy".to_owned(), "test".to_owned()],
        replay_command_ledger_digest: digest_b3_typed(b"gc1-qualification-ledger"),
        replay_observation_digest: digest_b3_typed(b"gc1-qualification-observation"),
        eligible_dependency_ids_digest: digest_b3_typed(b"gc1-qualification-eligible"),
        eligible_dependency_count: 3,
        unchanged_dependency_compilations: Some(0),
        covered_origin_acquisitions: Some(0),
        observation_gaps: Vec::new(),
        required_outputs_present: true,
        archive_digest_after_replay: frozen.archive_sha256.clone(),
        completed_coverage_digest: digest_b3_typed(b"gc1-qualification-completed"),
        qualification_outcome: QualificationOutcome::Pass,
    };
    verify_replay_frozen(&proof, frozen).map_err(|error| invalid(&error.to_string()))?;
    Ok(proof)
}

fn check_frozen_replay() -> Result<(), Box<dyn Error>> {
    let frozen = frozen_candidate()?;
    let proof = replay_proof(&frozen)?;
    if proof.eligible_dependency_count == 0 {
        return Err(invalid("replay lost its nonempty eligible set"));
    }
    let bytes = replay_canonical_bytes(&proof).map_err(|error| invalid(&error.to_string()))?;
    let parsed = parse_replay_bytes(&bytes).map_err(|error| invalid(&error.to_string()))?;
    if parsed != proof {
        return Err(invalid("frozen proof lost its strict round trip"));
    }
    if replay_digest(&proof).map_err(|error| invalid(&error.to_string()))?
        != replay_digest(&parsed).map_err(|error| invalid(&error.to_string()))?
    {
        return Err(invalid("frozen proof digest drifted across round trip"));
    }
    Ok(())
}

fn selectable_entry(generation: &GenerationId) -> SelectableEntry {
    SelectableEntry {
        cache_id: 7,
        repository_id: 1,
        ref_name: "refs/heads/main".to_owned(),
        key: generation.cache_key(),
        version: "v1".to_owned(),
        size_in_bytes: 512,
        created_at: "2026-10-25T00:00:00Z".to_owned(),
        provenance: EntryProvenance::Trusted,
    }
}

fn discovery_request_for(generation: GenerationId) -> Result<DiscoveryRequest, Box<dyn Error>> {
    Ok(DiscoveryRequest {
        job: fixture_job("reader")?,
        generation,
        expected_cache_version: "v1".to_owned(),
        allowed_refs: vec!["refs/heads/main".to_owned()],
        current_ref: "refs/heads/main".to_owned(),
        inventory_digest: digest_b3_typed(b"gc1-qualification-inventory"),
        inventory_complete: true,
        entries: vec![selectable_entry(&generation_for(
            b"gc1-qualification-coverage",
        )?)],
        active_coverage_ids: vec!["coverage-a".to_owned()],
    })
}

fn check_exact_discovery_first() -> Result<(), Box<dyn Error>> {
    let generation = generation_for(b"gc1-qualification-coverage")?;
    let request = discovery_request_for(generation.clone())?;
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("exact requested key lost first position"));
    }
    if record.shadow_state != ShadowState::Clear {
        return Err(invalid("clean inventory reported a shadow"));
    }
    let candidate = record
        .exact_candidate
        .as_ref()
        .ok_or_else(|| invalid("exact hit lost its candidate"))?;
    if candidate.entry.key != generation.cache_key() {
        return Err(invalid("exact hit kept the wrong candidate"));
    }
    let bytes = discovery_canonical_bytes(&record).map_err(|error| invalid(&error.to_string()))?;
    let parsed = parse_discovery_bytes(&bytes).map_err(|error| invalid(&error.to_string()))?;
    if parsed != record {
        return Err(invalid("discovery lost its strict round trip"));
    }
    if discovery_digest(&record).map_err(|error| invalid(&error.to_string()))?
        != discovery_digest(&parsed).map_err(|error| invalid(&error.to_string()))?
    {
        return Err(invalid("discovery digest drifted across round trip"));
    }
    let observation = RestoreObservation {
        intended_key: generation.cache_key(),
        matched_key: candidate.entry.key.clone(),
        restored_compatibility: generation.compatibility_digest.clone(),
        expected_compatibility: generation.compatibility_digest.clone(),
        restored_coverage: generation.coverage_digest.clone(),
        expected_coverage: generation.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-qualification-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-qualification-manifest"),
    };
    verify_restore(&observation).map_err(|error| invalid(&error.to_string()))?;
    Ok(())
}

pub(crate) fn declared_entry(path: &str, bytes: &[u8], mode: u32) -> ProjectionEntry {
    ProjectionEntry {
        path: path.to_owned(),
        kind: EntryKind::File,
        mode,
        content_sha256: sha256_hex(bytes),
        link_target: None,
        mtime: Some(RealMtime {
            seconds: 946_684_800,
            nanoseconds: 123_456_789,
        }),
    }
}

pub(crate) fn write_checkout_file(
    root: &Path,
    path: &str,
    bytes: &[u8],
    mode: u32,
) -> Result<PathBuf, Box<dyn Error>> {
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&full, bytes)?;
    fs::set_permissions(&full, fs::Permissions::from_mode(mode))?;
    Ok(full)
}

fn check_fresh_runner_restore() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("checkout");
    fs::create_dir_all(&checkout)?;
    let declared = declared_entry("src/lib.rs", b"restored-input", 0o644);
    declared.validate()?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Missing {
        return Err(invalid("empty fresh runner misclassified its state"));
    }
    write_checkout_file(&checkout, "src/lib.rs", b"restored-input", 0o644)?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Unchanged {
        return Err(invalid("restored input rejected on a fresh runner"));
    }
    if decide(Some(&declared), Some(&declared), CheckoutState::Unchanged)
        != RestoreDecision::Restore
    {
        return Err(invalid("unchanged input not scheduled for restore"));
    }
    Ok(())
}

fn check_rejects_false_warm() -> Result<(), Box<dyn Error>> {
    let frozen = frozen_candidate()?;
    let proof = replay_proof(&frozen)?;
    let mut wrong_digest = proof.clone();
    wrong_digest.frozen_archive_digest = "0".repeat(64);
    must_fail(
        &verify_replay_frozen(&wrong_digest, &frozen).map_err(|_| invalid("foreign")),
        "foreign frozen digest admitted",
    )?;
    let mut wrong_bytes = proof.clone();
    wrong_bytes.frozen_archive_bytes += 1;
    must_fail(
        &verify_replay_frozen(&wrong_bytes, &frozen).map_err(|_| invalid("drift")),
        "frozen byte count drift admitted",
    )?;
    let mut mutated = proof.clone();
    mutated.archive_digest_after_replay = "1".repeat(64);
    must_fail(
        &verify_replay_frozen(&mutated, &frozen).map_err(|_| invalid("mutated")),
        "replay-mutated archive admitted",
    )?;
    let mut compiled = proof.clone();
    compiled.unchanged_dependency_compilations = Some(2);
    must_fail(
        &verify_replay_frozen(&compiled, &frozen).map_err(|_| invalid("compiled")),
        "unchanged dependency compilation admitted as warm",
    )?;
    let mut unknown = proof.clone();
    unknown.unchanged_dependency_compilations = None;
    must_fail(
        &verify_replay_frozen(&unknown, &frozen).map_err(|_| invalid("unknown")),
        "unknown compilations admitted as warm zero",
    )?;
    let mut gaps = proof.clone();
    gaps.observation_gaps = vec!["native coverage incomplete".to_owned()];
    must_fail(
        &verify_replay_frozen(&gaps, &frozen).map_err(|_| invalid("gaps")),
        "gapped observation admitted as warm zero",
    )?;
    let generation = generation_for(b"gc1-qualification-coverage")?;
    let mut mismatch = RestoreObservation {
        intended_key: generation.cache_key(),
        matched_key: generation.cache_key(),
        restored_compatibility: generation.compatibility_digest.clone(),
        expected_compatibility: generation.compatibility_digest.clone(),
        restored_coverage: generation.coverage_digest.clone(),
        expected_coverage: generation.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-qualification-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-qualification-manifest"),
    };
    mismatch.matched_key = compat_variant_generation()?.cache_key();
    must_fail(
        &verify_restore(&mismatch).map_err(|_| invalid("key")),
        "mismatched restore key admitted",
    )?;
    let other = discovery_request_for(compat_variant_generation()?)?;
    let other_record = decide_discovery(&other).map_err(|error| invalid(&error.to_string()))?;
    if other_record.decision == DiscoveryDecision::ExactHit {
        return Err(invalid("incompatible generation kept the exact hit"));
    }
    Ok(())
}

#[test]
fn c02() -> Result<(), Box<dyn Error>> {
    require_nonempty_workload(&workload())?;
    require_complete_observation(&observation("frozen fresh-runner replay"))?;
    check_frozen_replay()?;
    check_exact_discovery_first()?;
    check_fresh_runner_restore()?;
    check_rejects_false_warm()?;
    Ok(())
}

pub(crate) fn check_app_only_preserves_coverage() -> Result<(), Box<dyn Error>> {
    let base = base_coverage()?;
    let app_only = coverage_variant("0.1.1", "0.2.0", None, LINUX_TARGET, None, 'f')?;
    require_valid_digests(&base)?;
    require_valid_digests(&app_only)?;
    require_same_coverage(&base, &app_only)?;
    let dep_bump = coverage_variant("0.1.0", "0.3.0", None, LINUX_TARGET, None, 'f')?;
    require_changed_coverage(&base, &dep_bump)?;
    if base.workspace_digest == dep_bump.workspace_digest {
        return Err(invalid("dependency change kept the workspace digest"));
    }
    Ok(())
}
