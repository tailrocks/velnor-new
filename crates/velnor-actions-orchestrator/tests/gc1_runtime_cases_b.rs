// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_frozen_snapshot_replay() -> Result<(), Box<dyn Error>> {
    let frozen = fixture_frozen()?;
    let proof = fixture_proof(&frozen)?;
    verify_replay_frozen(&proof, &frozen).map_err(|error| fail(&error.to_string()))?;
    if proof.ephemeral_mbx_initial_state != MbxInitialState::Empty {
        return Err(fail(
            "replay did not start from an empty ephemeral MBX store",
        ));
    }
    if proof.unchanged_dependency_compilations != Some(0) {
        return Err(fail("replay left unchanged dependency work unmeasured"));
    }
    let bytes = replay_canonical_bytes(&proof).map_err(|error| fail(&error.to_string()))?;
    if parse_replay_bytes(&bytes).map_err(|error| fail(&error.to_string()))? != proof {
        return Err(fail("frozen proof lost its strict round trip"));
    }
    if replay_digest(&proof).map_err(|error| fail(&error.to_string()))?
        != replay_digest(&parse_replay_bytes(&bytes).map_err(|error| fail(&error.to_string()))?)
            .map_err(|error| fail(&error.to_string()))?
    {
        return Err(fail("frozen proof digest drifted across round trip"));
    }
    let marker = b"gc1-runtime-frozen-candidate-marker:";
    if bytes.windows(marker.len()).any(|window| window == marker) {
        return Err(fail("proof carried frozen archive bytes"));
    }
    if proof.archive_digest_after_replay != frozen.archive_sha256 {
        return Err(fail("replay mutated the frozen candidate"));
    }
    Ok(())
}

fn fixture_manifest() -> Result<SnapshotManifestV1, Box<dyn Error>> {
    let mut raw = BTreeMap::new();
    raw.insert("projection".to_owned(), 512_u64);
    raw.insert("state".to_owned(), 1_048_576_u64);
    Ok(SnapshotManifestV1 {
        schema: 1,
        generation: fixture_generation(b"gc1-runtime-coverage")?,
        producer_job: fixture_job("writer")?,
        source_binding_digest: digest_b3_typed(b"gc1-runtime-source"),
        claimed_completed_recipe_ids: vec![
            "check".to_owned(),
            "clippy".to_owned(),
            "test".to_owned(),
        ],
        requested_coverage_digest: digest_b3_typed(b"gc1-runtime-requested"),
        compatibility_digest: digest_b3_typed(b"gc1-runtime-compat"),
        paired_source_generation: None,
        paired_tool_generation: None,
        paired_source_archive_digest: None,
        paired_tool_archive_digest: None,
        lane_roots: vec!["state/build".to_owned(), "state/test".to_owned()],
        projection_index_digest: digest_b3_typed(b"gc1-runtime-projection"),
        state_index_digest: digest_b3_typed(b"gc1-runtime-state"),
        required_output_index_digest: digest_b3_typed(b"gc1-runtime-required"),
        archive_format: SNAPSHOT_ARCHIVE_FORMAT.to_owned(),
        mtime_precision: SNAPSHOT_MTIME_PRECISION.to_owned(),
        excluded_path_policy_digest: digest_b3_typed(b"gc1-runtime-excluded"),
        raw_bytes_by_component: raw,
        entry_count: 7,
    })
}

fn check_snapshot_manifest_freezes_claim() -> Result<(), Box<dyn Error>> {
    let manifest = fixture_manifest()?;
    manifest.validate()?;
    if manifest.claimed_completed_recipe_ids.len() != 3 {
        return Err(fail("snapshot claim lost a completed recipe"));
    }
    let bytes = manifest_canonical_bytes(&manifest)?;
    if parse_manifest_bytes(&bytes)? != manifest {
        return Err(fail("snapshot manifest lost its strict round trip"));
    }
    Ok(())
}

fn inventory_entry(cache_id: u64, key: &str, bytes: u64) -> EntryTupleV1 {
    EntryTupleV1 {
        repository_id: 1,
        cache_id,
        ref_name: "refs/heads/main".to_owned(),
        key: key.to_owned(),
        version: "v1".to_owned(),
        size_in_bytes: bytes,
        created_at: "2026-10-25T00:00:00Z".to_owned(),
        last_accessed_at: "2026-10-25T01:00:00Z".to_owned(),
    }
}

pub(crate) fn hosted_request(
    inventory_sum: u64,
    bound: u64,
) -> Result<AdmissionRequest, Box<dyn Error>> {
    let generation = fixture_generation(b"gc1-runtime-coverage")?;
    let entries = vec![
        inventory_entry(3, "velnor-gc1-state-old-baseline", 400),
        inventory_entry(7, &generation.cache_key(), inventory_sum - 400),
    ];
    let inventory = build_inventory(1, "2026-10-25T02:00:00Z", entries, 2)?;
    let digest = inventory_digest(&inventory)?;
    let inner = freeze_inner_payload(&velnor_actions_orchestrator::PackedArchive {
        archive_sha256: sha256_hex(b"gc1-runtime-inner"),
        archive_bytes: 1024,
        member_count: 4,
    })?;
    let measured = measure_envelope(
        &inner,
        &velnor_actions_orchestrator::PreflightMeasurement {
            measured_outer_bytes: bound,
            outer_sha256: sha256_hex(b"gc1-runtime-outer"),
        },
        0,
        &velnor_actions_orchestrator::OuterEnvelopeIdentity {
            tool_identity: "tar-0.4.46+flate2-1.1.10".to_owned(),
            transport_member: "payload.tar.gz".to_owned(),
        },
        true,
    )?;
    Ok(AdmissionRequest {
        job: fixture_job("writer")?,
        generation,
        active_intent_digest: digest_b3_typed(b"gc1-runtime-intent"),
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
        envelope: OuterEnvelope::Measured(measured),
        occupied_generation_slots: 1,
        peak_local_disk_bound: measured_bytes(1_000_000),
        available_local_disk: measured_bytes(100_000_000),
        local_disk_reserve: 1_000_000,
        claimed_freed_bytes: 0,
    })
}

pub(crate) fn fixture_draft(head: &str) -> IntentDraft {
    IntentDraft {
        repository_id: 1,
        default_ref: "refs/heads/main".to_owned(),
        observed_head_sha: head.to_owned(),
        configuration_digest: digest_b3_typed(b"gc1-runtime-config"),
        active_group_ids: vec!["group-a".to_owned()],
        active_coverage_ids: vec!["coverage-a".to_owned()],
        derived_at: "2026-10-26T00:00:00Z".to_owned(),
        derivation_evidence_digest: digest_b3_typed(b"gc1-runtime-evidence"),
    }
}

fn check_all_family_admission_and_intent() -> Result<(), Box<dyn Error>> {
    let request = hosted_request(4_200, 1_000_000)?;
    let admitted = decide_admission(&request)?;
    if admitted.decision != AdmissionDecision::Admitted {
        return Err(fail("complete all-family inventory missed admission"));
    }
    if !admitted.blocked_reasons.is_empty() {
        return Err(fail("admitted candidate kept block reasons"));
    }
    let bytes = admission_canonical_bytes(&admitted)?;
    if parse_admission_bytes(&bytes)? != admitted {
        return Err(fail("admission lost its strict round trip"));
    }
    let intent = derive_intent(fixture_draft(&format!("{:040x}", 0x5a5a)))?;
    let observation = IntentObservation {
        observed_head_sha: format!("{:040x}", 0x5a5a),
        configuration_digest: digest_b3_typed(b"gc1-runtime-config"),
        active_group_ids: vec!["group-a".to_owned()],
        active_coverage_ids: vec!["coverage-a".to_owned()],
    };
    let recheck = recheck_intent(&intent, Some(&observation))?;
    if !intent_authorizes_mutation(recheck) {
        return Err(fail("unchanged active intent blocked mutation"));
    }
    let intent_bytes = intent_canonical_bytes(&intent)?;
    if parse_intent_bytes(&intent_bytes)? != intent {
        return Err(fail("active intent lost its strict round trip"));
    }
    if intent_digest(&intent)? != intent_digest(&parse_intent_bytes(&intent_bytes)?)? {
        return Err(fail("active intent digest drifted across round trip"));
    }
    Ok(())
}

fn check_bounded_repair_key() -> Result<(), Box<dyn Error>> {
    let base = fixture_generation(b"gc1-runtime-coverage")?;
    GenerationId::validate_key(&base.cache_key())?;
    if !base.cache_key().ends_with("-r0") {
        return Err(fail("base generation lost its zero repair revision"));
    }
    let repaired = GenerationId {
        repair_revision: 1,
        ..base.clone()
    };
    GenerationId::validate_key(&repaired.cache_key())?;
    if repaired.cache_key() == base.cache_key() {
        return Err(fail("repair revision did not change the cache key"));
    }
    if !repaired.cache_key().ends_with("-r1") {
        return Err(fail("repair revision drifted from its bounded key"));
    }
    if GenerationId::parse_key(&repaired.cache_key())? != repaired {
        return Err(fail("repair key lost its strict round trip"));
    }
    Ok(())
}

pub(crate) fn selectable(
    cache_id: u64,
    key: &str,
    ref_name: &str,
    provenance: EntryProvenance,
) -> SelectableEntry {
    SelectableEntry {
        cache_id,
        repository_id: 1,
        ref_name: ref_name.to_owned(),
        key: key.to_owned(),
        version: "v1".to_owned(),
        size_in_bytes: 512,
        created_at: "2026-10-25T00:00:00Z".to_owned(),
        provenance,
    }
}

fn check_exact_discovery_first() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation(b"gc1-runtime-coverage")?;
    let request = DiscoveryRequest {
        job: fixture_job("reader")?,
        generation: generation.clone(),
        expected_cache_version: "v1".to_owned(),
        allowed_refs: vec!["refs/heads/main".to_owned()],
        current_ref: "refs/heads/main".to_owned(),
        inventory_digest: digest_b3_typed(b"gc1-runtime-inventory"),
        inventory_complete: true,
        entries: vec![selectable(
            7,
            &generation.cache_key(),
            "refs/heads/main",
            EntryProvenance::Trusted,
        )],
        active_coverage_ids: vec!["coverage-a".to_owned()],
    };
    let record = decide_discovery(&request)?;
    if record.decision != DiscoveryDecision::ExactHit {
        return Err(fail("exact requested key lost first position"));
    }
    if record.shadow_state != ShadowState::Clear {
        return Err(fail("clean inventory reported a shadow"));
    }
    let candidate = record
        .exact_candidate
        .as_ref()
        .ok_or_else(|| fail("exact hit lost its candidate"))?;
    if candidate.entry.key != generation.cache_key() {
        return Err(fail("exact hit kept the wrong candidate"));
    }
    let bytes = discovery_canonical_bytes(&record)?;
    if parse_discovery_bytes(&bytes)? != record {
        return Err(fail("discovery lost its strict round trip"));
    }
    if discovery_digest(&record)? != discovery_digest(&parse_discovery_bytes(&bytes)?)? {
        return Err(fail("discovery digest drifted across round trip"));
    }
    let observation = RestoreObservation {
        intended_key: generation.cache_key(),
        matched_key: candidate.entry.key.clone(),
        restored_compatibility: generation.compatibility_digest.clone(),
        expected_compatibility: generation.compatibility_digest.clone(),
        restored_coverage: generation.coverage_digest.clone(),
        expected_coverage: generation.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-runtime-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-runtime-manifest"),
    };
    verify_restore(&observation)?;
    Ok(())
}

#[test]
fn gc1_runtime_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_fresh_checkout_reuses_unchanged_inputs()?;
    check_changed_and_deleted_inputs()?;
    check_lane_context_uses_separate_build_dirs()?;
    check_observer_attributes_runtime_work()?;
    check_frozen_snapshot_replay()?;
    check_snapshot_manifest_freezes_claim()?;
    check_all_family_admission_and_intent()?;
    check_bounded_repair_key()?;
    check_exact_discovery_first()?;
    Ok(())
}
