// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_rejects_unsafe_source_restoration() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("checkout");
    let staged = temp.path().join("staged");
    fs::create_dir_all(&checkout)?;
    fs::create_dir_all(&staged)?;
    let declared = file_entry("src/lib.rs", b"declared", 0o644);
    write_checkout_file(&checkout, "src/lib.rs", b"other bytes", 0o644)?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Changed {
        return Err(fail("content mismatch missed"));
    }
    write_checkout_file(&checkout, "src/lib.rs", b"declared", 0o600)?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Changed {
        return Err(fail("mode mismatch missed"));
    }
    let mut tampered = declared.clone();
    tampered.content_sha256 = sha256_hex(b"tampered");
    must_fail!(reconcile(&tampered, &declared), "archive mismatch admitted");
    let mut timeless = declared.clone();
    timeless.mtime = None;
    must_fail!(reconcile(&timeless, &declared), "missing mtime admitted");
    let staged_file = write_checkout_file(&staged, "src/lib.rs", b"tampered", 0o644)?;
    let destination = staged_path(&checkout, "src/lib.rs")?;
    must_fail!(
        install_staged_file(&staged_file, &destination, &declared),
        "tampered staged file installed",
    );
    if fs::read(&destination)? != b"declared" {
        return Err(fail("failed install wrote destination"));
    }
    let wrong_stamp = RealMtime {
        seconds: ARCHIVED_SECONDS,
        nanoseconds: 999_999_999,
    };
    must_fail!(
        verify_times(&checkout.join("src/lib.rs"), &wrong_stamp),
        "wrong mtime verified",
    );
    must_fail!(
        validate_link_target_in_root(Path::new("/checkout"), "config/link", "../../escape"),
        "escaping link admitted",
    );
    Ok(())
}

fn check_rejects_shared_context_roots() -> Result<(), Box<dyn Error>> {
    let mut shared = fixture_context()?;
    shared.cargo_home = shared.owned_root.clone();
    must_fail!(shared.validate(), "shared owned roots admitted");
    let mut lanes = fixture_context()?;
    lanes.lanes.clear();
    must_fail!(lanes.validate(), "laneless context admitted");
    let mut duplicate = fixture_context()?;
    let first_lane = duplicate.lanes[0].lane_id.clone();
    duplicate.lanes[1].lane_id = first_lane;
    must_fail!(duplicate.validate(), "duplicate lane admitted");
    let mut views = fixture_context()?;
    views.mbx_target_views = true;
    must_fail!(
        views.validate(),
        "target views admitted in snapshot context"
    );
    let mut adapters = fixture_context()?;
    adapters.compiler_adapters.clear();
    must_fail!(adapters.validate(), "adapterless context admitted");
    Ok(())
}

fn check_rejects_incomplete_observation() -> Result<(), Box<dyn Error>> {
    let mut first = ObservationSession::begin(
        "internal/lint/dev",
        "gc1-runtime-check",
        &new_session_id(),
        &digest_b3(b"gc1-runtime-observation-root"),
    )?;
    let second = ObservationSession::begin(
        "internal/lint/dev",
        "gc1-runtime-check",
        &new_session_id(),
        &digest_b3(b"gc1-runtime-observation-root"),
    )?;
    let foreign = second.compiler_evidence(None, None, None, None, None, "detail")?;
    must_fail!(
        first.provide_compiler(foreign),
        "foreign session evidence admitted",
    );
    let mut native_ledger = runtime_native()?;
    native_ledger.record_unobserved_absolute("/usr/bin/cc")?;
    if native_ledger.measured_native_compilations().is_some() {
        return Err(fail("unobserved native calls measured as zero"));
    }
    let mut partial = ObservationSession::begin(
        "internal/lint/dev",
        "gc1-runtime-check",
        &new_session_id(),
        &digest_b3(b"gc1-runtime-observation-root"),
    )?;
    let compiler = partial.compiler_evidence(None, None, None, None, None, "missing ledger")?;
    partial.provide_compiler(compiler)?;
    must_fail!(
        partial.finish(ChildOutcome::Success),
        "report finished with missing dimensions",
    );
    Ok(())
}

fn check_rejects_unproven_replay() -> Result<(), Box<dyn Error>> {
    let frozen = fixture_frozen()?;
    let proof = fixture_proof(&frozen)?;
    let mut wrong_digest = proof.clone();
    wrong_digest.frozen_archive_digest = "0".repeat(64);
    must_fail!(
        verify_replay_frozen(&wrong_digest, &frozen),
        "foreign frozen digest admitted",
    );
    let mut wrong_bytes = proof.clone();
    wrong_bytes.frozen_archive_bytes += 1;
    must_fail!(
        verify_replay_frozen(&wrong_bytes, &frozen),
        "frozen byte count drift admitted",
    );
    let mut mutated = proof.clone();
    mutated.archive_digest_after_replay = "1".repeat(64);
    must_fail!(
        verify_replay_frozen(&mutated, &frozen),
        "replay-mutated archive admitted",
    );
    let other = freeze_archive(b"gc1-runtime-other-candidate")?;
    must_fail!(
        verify_replay_frozen(&proof, &other),
        "proof verified against another archive",
    );
    let mut compiled = proof.clone();
    compiled.unchanged_dependency_compilations = Some(2);
    must_fail!(
        verify_replay_frozen(&compiled, &frozen),
        "unchanged dependency compilation admitted",
    );
    let mut unknown_compilations = proof.clone();
    unknown_compilations.unchanged_dependency_compilations = None;
    must_fail!(
        verify_replay_frozen(&unknown_compilations, &frozen),
        "unknown compilations admitted as zero",
    );
    let mut gaps = proof.clone();
    gaps.observation_gaps = vec!["native coverage incomplete".to_owned()];
    must_fail!(
        verify_replay_frozen(&gaps, &frozen),
        "open observation gaps admitted",
    );
    let mut timeless = proof.clone();
    timeless.metadata_roundtrip_passed = false;
    must_fail!(
        verify_replay_frozen(&timeless, &frozen),
        "failed metadata round trip admitted",
    );
    let mut missing = proof.clone();
    missing.required_outputs_present = false;
    must_fail!(
        verify_replay_frozen(&missing, &frozen),
        "missing required outputs admitted",
    );
    Ok(())
}

fn check_rejects_blocked_publication() -> Result<(), Box<dyn Error>> {
    let mut unknown_inventory = hosted_request(4_200, 1_000_000)?;
    unknown_inventory.inventory_bytes = unknown_bytes("pagination blocked")?;
    let blocked = decide_admission(&unknown_inventory)?;
    if blocked.decision != AdmissionDecision::Blocked
        || !blocked
            .blocked_reasons
            .contains(&BLOCKED_UNKNOWN_INVENTORY.to_owned())
    {
        return Err(fail("unknown inventory missed its block"));
    }
    let mut foreign = hosted_request(4_200, 1_000_000)?;
    foreign.foreign_inflight_bytes = unknown_bytes("foreign writers unbounded")?;
    let blocked = decide_admission(&foreign)?;
    if !blocked
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_FOREIGN_EXPOSURE.to_owned())
    {
        return Err(fail("unknown foreign exposure missed its block"));
    }
    let mut envelope = hosted_request(4_200, 1_000_000)?;
    envelope.envelope = OuterEnvelope::Unknown(unknown_envelope(
        UNKNOWN_TOOL_IDENTITY,
        "pinned tool behavior not yet qualified",
    )?);
    let blocked = decide_admission(&envelope)?;
    if !blocked
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_RUNTIME_ENVELOPE.to_owned())
    {
        return Err(fail("unknown runtime envelope missed its block"));
    }
    let mut tight = hosted_request(4_200, OPERATING_CEILING_BYTES)?;
    let blocked = decide_admission(&tight)?;
    if blocked.decision != AdmissionDecision::Blocked
        || !blocked
            .blocked_reasons
            .contains(&BLOCKED_INSUFFICIENT_HEADROOM.to_owned())
    {
        return Err(fail("over-ceiling candidate missed its headroom block"));
    }
    tight.occupied_generation_slots = 2;
    let blocked = decide_admission(&tight)?;
    if !blocked
        .blocked_reasons
        .contains(&BLOCKED_GENERATION_SLOTS_EXHAUSTED.to_owned())
    {
        return Err(fail("full generation slots missed their block"));
    }
    Ok(())
}

fn check_rejects_stale_intent_and_shadow() -> Result<(), Box<dyn Error>> {
    let head = format!("{:040x}", 0x5a5a);
    let intent = derive_intent(fixture_draft(&head))?;
    let moved = recheck_intent(
        &intent,
        Some(&IntentObservation {
            observed_head_sha: format!("{:040x}", 0xa5a5),
            configuration_digest: digest_b3_typed(b"gc1-runtime-config"),
            active_group_ids: vec!["group-a".to_owned()],
            active_coverage_ids: vec!["coverage-b".to_owned()],
        }),
    )?;
    if intent_authorizes_mutation(moved) {
        return Err(fail("stale coverage intent authorized mutation"));
    }
    if intent_authorizes_mutation(recheck_intent(&intent, None)?) {
        return Err(fail("unknown intent authorized mutation"));
    }
    let generation = fixture_generation(b"gc1-runtime-coverage")?;
    let shadowed = DiscoveryRequest {
        job: fixture_job("writer")?,
        generation: generation.clone(),
        expected_cache_version: "v1".to_owned(),
        allowed_refs: vec!["refs/heads/main".to_owned(), "refs/pull/9/merge".to_owned()],
        current_ref: "refs/heads/main".to_owned(),
        inventory_digest: digest_b3_typed(b"gc1-runtime-inventory"),
        inventory_complete: true,
        entries: vec![
            selectable(
                7,
                &generation.cache_key(),
                "refs/heads/main",
                EntryProvenance::Trusted,
            ),
            selectable(
                9,
                &generation.cache_key(),
                "refs/pull/9/merge",
                EntryProvenance::Untrusted,
            ),
        ],
        active_coverage_ids: vec!["coverage-a".to_owned()],
    };
    let refused = decide_discovery(&shadowed)?;
    if refused.decision != DiscoveryDecision::RefusedShadow
        || refused.shadow_state != ShadowState::ConflictingVisibleRef
    {
        return Err(fail("conflicting visible ref escaped the shadow guard"));
    }
    let prefix_mismatch = RestoreObservation {
        intended_key: generation.cache_key(),
        matched_key: format!("{}-suffix", generation.cache_key()),
        restored_compatibility: generation.compatibility_digest.clone(),
        expected_compatibility: generation.compatibility_digest.clone(),
        restored_coverage: generation.coverage_digest.clone(),
        expected_coverage: generation.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-runtime-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-runtime-manifest"),
    };
    must_fail!(
        verify_restore(&prefix_mismatch),
        "prefix-only restore admitted",
    );
    Ok(())
}

fn check_selection_helpers_cover_exact_shadow_predecessor() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation(b"gc1-runtime-coverage")?;
    let prefix = primary_prefix(&generation);
    if generation.cache_key() != format!("{prefix}-r0") {
        return Err(fail("primary prefix drifted from cache key"));
    }
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
    let visible: Vec<&SelectableEntry> = request.entries.iter().collect();
    let exact =
        select_exact(&request, &visible).ok_or_else(|| fail("direct exact selection missed"))?;
    if exact.entry.key != generation.cache_key() {
        return Err(fail("direct exact kept the wrong candidate"));
    }
    if select_predecessor(&request, &visible).is_some() {
        return Err(fail("exact-only history produced a predecessor"));
    }
    let exact_refs = std::collections::BTreeSet::from(["refs/heads/main".to_owned()]);
    let longer_refs = std::collections::BTreeSet::new();
    if classify_shadow(&exact_refs, &longer_refs, "refs/heads/main") != ShadowState::Clear {
        return Err(fail("clean exact reported a shadow"));
    }
    let conflicting = std::collections::BTreeSet::from([
        "refs/heads/main".to_owned(),
        "refs/pull/9/merge".to_owned(),
    ]);
    if classify_shadow(&exact_refs, &conflicting, "refs/heads/main")
        != ShadowState::ConflictingVisibleRef
    {
        return Err(fail("conflicting longer refs escaped the shadow guard"));
    }
    Ok(())
}

#[test]
fn gc1_runtime_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_unsafe_source_restoration()?;
    check_rejects_shared_context_roots()?;
    check_rejects_incomplete_observation()?;
    check_rejects_unproven_replay()?;
    check_rejects_blocked_publication()?;
    check_rejects_stale_intent_and_shadow()?;
    check_selection_helpers_cover_exact_shadow_predecessor()?;
    Ok(())
}
