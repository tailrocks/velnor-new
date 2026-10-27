// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// C17: only legitimate missing closure work compiles while unchanged covered
/// dependencies report warm zeros for the enlarged generation.
pub(crate) fn check_growth_compiles_only_missing() -> Result<(), Box<dyn Error>> {
    let changed = changed_closure_ledger()?;
    if changed.dependency_compilations() != 1 {
        return Err(invalid(
            "growth compiled more than the missing closure work",
        ));
    }
    if changed.application_compilations() != 0 {
        return Err(invalid("growth recompiled the unchanged application"));
    }
    if changed.failed() != 0 {
        return Err(invalid("growth ledger recorded failures"));
    }
    let warm = warm_native_ledger();
    require_measured_zero(
        "unchanged covered native work",
        warm.measured_native_compilations(),
    )?;
    let cold = cold_ledger()?;
    must_fail(
        &require_measured_zero(
            "changed closure as warm zero",
            Some(cold.dependency_compilations()),
        ),
        "changed closure work admitted as warm zero",
    )?;
    Ok(())
}

/// C17: the new coverage generation publishes only after complete preparation
/// and a frozen-archive replay; repeated earlier command success alone never
/// authorizes publication.
pub(crate) fn check_new_generation_publishes_after_replay() -> Result<(), Box<dyn Error>> {
    let frozen = failure_frozen()?;
    let job = failure_job("push", "refs/heads/main", 172, 1, 1721, "writer")?;
    let proof = failure_proof(
        &frozen,
        b"gc1-qualification-failure-coverage-new",
        job.clone(),
    )?;
    if proof.eligible_dependency_count == 0 {
        return Err(invalid("growth replay lost its nonempty eligible set"));
    }
    let truncated = freeze_archive(&failure_frozen_bytes()[..failure_frozen_bytes().len() - 1])
        .map_err(|error| invalid(&error.to_string()))?;
    must_fail(
        &verify_replay_frozen(&proof, &truncated),
        "partial preparation admitted as complete",
    )?;
    let save = failure_save(true, None, true, false, Some(1024), 8192, SaveHalt::Running);
    if decide_save(&save)? != SaveOutcome::Attempted {
        return Err(invalid("prepared growth blocked admission"));
    }
    let bytes = failure_frozen_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let generation = failure_generation(b"gc1-qualification-failure-coverage-new")?;
    if generation.cache_key() != proof.generation.cache_key() {
        return Err(invalid("growth publication drifted from its replay"));
    }
    let published = PublicationV1 {
        schema: PUBLICATION_SCHEMA,
        job,
        generation: generation.clone(),
        frozen_archive_digest: digest.clone(),
        manifest_digest: digest_b3_typed(b"gc1-qualification-failure-manifest"),
        frozen_proof_digest: digest_b3_typed(b"gc1-qualification-failure-proof"),
        admission_digest: digest_b3_typed(b"gc1-qualification-failure-admission"),
        save_step_outcome: Some("success".to_owned()),
        save_outcome: SaveOutcome::Attempted,
        observed_entry: Some(ObservedEntry {
            repository_id: 1,
            cache_id: 1721,
            ref_name: "refs/heads/main".to_owned(),
            key: generation.cache_key(),
            version: "v1".to_owned(),
            size_in_bytes: size,
            created_at: "2026-10-26T00:00:00Z".to_owned(),
        }),
        readback_outcome: ReadbackOutcome::Verified,
        readback_archive_digest: Some(digest),
        readback_manifest_digest: Some(digest_b3_typed(b"gc1-qualification-failure-manifest")),
        actual_stored_bytes: measured_bytes(size),
        active_intent_digest: digest_b3_typed(b"gc1-qualification-failure-intent"),
    };
    if classify_publication(&published)? != PublicationOutcome::Verified {
        return Err(invalid("prepared growth lost its verified publication"));
    }
    Ok(())
}

/// C17: the immutable old key is never overwritten, superseded coverage is
/// removed only after verified replacement, and an older queued plan cannot
/// retire the currently active generation.
pub(crate) fn check_old_key_immutable_until_replaced() -> Result<(), Box<dyn Error>> {
    let old_generation = failure_generation(b"gc1-qualification-failure-coverage-old")?;
    let new_generation = failure_generation(b"gc1-qualification-failure-coverage-new")?;
    let job = failure_job("push", "refs/heads/main", 173, 1, 1731, "reader")?;
    let request = failure_discovery(
        job,
        old_generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![
            failure_entry(
                &old_generation,
                "refs/heads/main",
                1730,
                EntryProvenance::Trusted,
            ),
            failure_entry(
                &new_generation,
                "refs/heads/main",
                1731,
                EntryProvenance::Trusted,
            ),
        ],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("immutable old key lost its hit"));
    }
    let candidate = record
        .exact_candidate
        .as_ref()
        .ok_or_else(|| invalid("old key lost its candidate"))?;
    if candidate.entry.cache_id != 1730 {
        return Err(invalid("old key resolved to the replacement entry"));
    }
    let retired = retain_usable_families(
        &[old_generation.cache_key(), new_generation.cache_key()],
        &old_generation.cache_key(),
    )?;
    if retired != vec![new_generation.cache_key()] {
        return Err(invalid("superseded coverage survived verified replacement"));
    }
    let queued =
        retain_usable_families(&[new_generation.cache_key()], &old_generation.cache_key())?;
    if queued != vec![new_generation.cache_key()] {
        return Err(invalid("older queued plan retired the active generation"));
    }
    Ok(())
}

/// C17: the pure current-intent check permits an application-only HEAD change
/// under unchanged active compatibility and coverage, and skips mutation when
/// the check is unavailable or the coverage retired.
pub(crate) fn check_current_intent_gates_mutation() -> Result<(), Box<dyn Error>> {
    let coverage_id = digest_b3(b"gc1-qualification-failure-coverage-new");
    let previous = derive_intent(failure_intent(
        &format!("{:040x}", 0xa5a5),
        vec![coverage_id.clone()],
    ))
    .map_err(|error| invalid(&error.to_string()))?;
    let canonical =
        intent_canonical_bytes(&previous).map_err(|error| invalid(&error.to_string()))?;
    let parsed = parse_intent_bytes(&canonical).map_err(|error| invalid(&error.to_string()))?;
    if parsed != previous {
        return Err(invalid("current intent lost its canonical round trip"));
    }
    if intent_digest(&previous).map_err(|error| invalid(&error.to_string()))?
        != intent_digest(&parsed).map_err(|error| invalid(&error.to_string()))?
    {
        return Err(invalid("current intent digest drifted across round trip"));
    }
    let unchanged = IntentObservation {
        observed_head_sha: format!("{:040x}", 0x5a5a),
        configuration_digest: digest_b3_typed(b"gc1-qualification-failure-config"),
        active_group_ids: vec![digest_b3(b"gc1-qualification-failure-group")],
        active_coverage_ids: vec![coverage_id.clone()],
    };
    unchanged
        .validate()
        .map_err(|error| invalid(&error.to_string()))?;
    let recheck =
        recheck_intent(&previous, Some(&unchanged)).map_err(|error| invalid(&error.to_string()))?;
    if recheck != IntentRecheck::CoverageUnchanged {
        return Err(invalid("application-only HEAD change lost its intent"));
    }
    if !intent_authorizes_mutation(recheck) {
        return Err(invalid("unchanged intent blocked its mutation"));
    }
    let skipped = failure_save(true, None, true, true, Some(1024), 8192, SaveHalt::Running);
    if decide_save(&skipped)? != SaveOutcome::SkippedExisting {
        return Err(invalid("current-intent permit repeated preparation"));
    }
    let unavailable =
        recheck_intent(&previous, None).map_err(|error| invalid(&error.to_string()))?;
    if unavailable != IntentRecheck::MutationBlocked {
        return Err(invalid("unavailable intent check permitted mutation"));
    }
    if intent_authorizes_mutation(unavailable) {
        return Err(invalid("blocked intent authorized mutation"));
    }
    let retired = IntentObservation {
        observed_head_sha: format!("{:040x}", 0x5a5a),
        configuration_digest: digest_b3_typed(b"gc1-qualification-failure-config"),
        active_group_ids: vec![digest_b3(b"gc1-qualification-failure-group")],
        active_coverage_ids: vec![digest_b3(b"gc1-qualification-failure-coverage-old")],
    };
    let recheck_retired =
        recheck_intent(&previous, Some(&retired)).map_err(|error| invalid(&error.to_string()))?;
    if recheck_retired != IntentRecheck::CoverageChanged {
        return Err(invalid("retired coverage kept its current intent"));
    }
    if intent_authorizes_mutation(recheck_retired) {
        return Err(invalid("retired coverage authorized mutation"));
    }
    Ok(())
}
