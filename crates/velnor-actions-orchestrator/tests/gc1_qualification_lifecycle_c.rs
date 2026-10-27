// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// C10: simultaneous runs request the same missing generation with explicit
/// nonempty workloads.
pub(crate) fn check_competing_requests_share_missing() -> Result<(), Box<dyn Error>> {
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-competing")?;
    let mut decisions = Vec::new();
    for (run_id, job_id) in [(211_u64, 2111_u64), (212_u64, 2121_u64)] {
        let job = lifecycle_job("push", "refs/heads/main", run_id, 1, job_id, "writer")?;
        let request = lifecycle_discovery(
            job,
            generation.clone(),
            vec!["refs/heads/main"],
            "refs/heads/main",
            Vec::new(),
            true,
        );
        let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
        if record.decision != DiscoveryDecision::ColdMissing {
            return Err(invalid("competing request missed the shared miss"));
        }
        decisions.push(record.decision);
    }
    if decisions.len() != 2 {
        return Err(invalid("competing requests lost a queued decision"));
    }
    Ok(())
}

/// C10: the first verified writer wins; the later writer reuses the verified
/// generation without repeating preparation, keeping one result per unit.
pub(crate) fn check_first_verified_writer_wins() -> Result<(), Box<dyn Error>> {
    let seed = b"gc1-qualification-lifecycle-competing";
    let generation = lifecycle_generation(seed)?;
    let first_job = lifecycle_job("push", "refs/heads/main", 211, 1, 2111, "writer")?;
    let frozen = lifecycle_frozen()?;
    lifecycle_proof(&frozen, seed, first_job)?;
    let second_save = lifecycle_save(true, None, true, true, SaveHalt::Running);
    if decide_save(&second_save)? != SaveOutcome::SkippedExisting {
        return Err(invalid("second writer repeated the verified preparation"));
    }
    let second_job = lifecycle_job("push", "refs/heads/main", 212, 1, 2121, "writer")?;
    let recheck = lifecycle_discovery(
        second_job,
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![
            lifecycle_entry(
                &generation,
                "refs/heads/main",
                2111,
                EntryProvenance::Trusted,
            ),
            lifecycle_entry(
                &generation,
                "refs/heads/main",
                2121,
                EntryProvenance::Trusted,
            ),
        ],
        true,
    );
    let recheck_record = decide_discovery(&recheck).map_err(|error| invalid(&error.to_string()))?;
    if recheck_record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("queued recheck missed the verified generation"));
    }
    let visible: Vec<&SelectableEntry> = recheck.entries.iter().collect();
    let candidate = select_exact(&recheck, &visible)
        .ok_or_else(|| invalid("recheck lost its single candidate"))?;
    if candidate.entry.cache_id != 2111 {
        return Err(invalid("recheck kept more than one selected unit"));
    }
    Ok(())
}

fn lifecycle_verified_publication() -> Result<PublicationV1, Box<dyn Error>> {
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-replacement")?;
    let bytes = lifecycle_frozen_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    Ok(PublicationV1 {
        schema: PUBLICATION_SCHEMA,
        job: lifecycle_job("push", "refs/heads/main", 111, 1, 1111, "writer")?,
        generation: generation.clone(),
        frozen_archive_digest: digest.clone(),
        manifest_digest: digest_b3_typed(b"gc1-qualification-lifecycle-manifest"),
        frozen_proof_digest: digest_b3_typed(b"gc1-qualification-lifecycle-proof"),
        admission_digest: digest_b3_typed(b"gc1-qualification-lifecycle-admission"),
        save_step_outcome: Some("success".to_owned()),
        save_outcome: SaveOutcome::Attempted,
        observed_entry: Some(ObservedEntry {
            repository_id: 1,
            cache_id: 1111,
            ref_name: "refs/heads/main".to_owned(),
            key: generation.cache_key(),
            version: "v1".to_owned(),
            size_in_bytes: size,
            created_at: "2026-10-26T00:00:00Z".to_owned(),
        }),
        readback_outcome: ReadbackOutcome::Verified,
        readback_archive_digest: Some(digest),
        readback_manifest_digest: Some(digest_b3_typed(b"gc1-qualification-lifecycle-manifest")),
        actual_stored_bytes: MeasuredU64 {
            value: Some(size),
            unknown_reason: None,
        },
        active_intent_digest: digest_b3_typed(b"gc1-qualification-lifecycle-intent"),
    })
}

/// C11: cancelled preparation never completes and never saves.
pub(crate) fn check_preparation_cancel_never_completes() -> Result<(), Box<dyn Error>> {
    let partial = lifecycle_save(true, None, false, false, SaveHalt::Running);
    if decide_save(&partial)? != SaveOutcome::Denied {
        return Err(invalid("partial preparation escaped its denial"));
    }
    let interrupted_prep = lifecycle_save(true, None, false, false, SaveHalt::Interrupted);
    if decide_save(&interrupted_prep)? != SaveOutcome::Denied {
        return Err(invalid("cancelled preparation marked complete"));
    }
    let frozen = lifecycle_frozen()?;
    let full_bytes = lifecycle_frozen_bytes();
    let truncated = freeze_archive(&full_bytes[..full_bytes.len() - 1])
        .map_err(|error| invalid(&error.to_string()))?;
    let job = lifecycle_job("push", "refs/heads/main", 111, 1, 1111, "writer")?;
    let partial_proof = lifecycle_proof(&frozen, b"gc1-qualification-lifecycle-partial", job)?;
    must_fail(
        &verify_replay_frozen(&partial_proof, &truncated),
        "partial preparation admitted as complete",
    )?;
    Ok(())
}

/// C11: cancelled upload marks the save interrupted without inferring
/// publication from a successful setup step.
pub(crate) fn check_upload_cancel_marks_interrupted() -> Result<(), Box<dyn Error>> {
    let cancelled = lifecycle_save(true, None, true, false, SaveHalt::Interrupted);
    if decide_save(&cancelled)? != SaveOutcome::Interrupted {
        return Err(invalid("cancelled upload escaped its interruption"));
    }
    let record = PublicationV1 {
        save_step_outcome: Some("success".to_owned()),
        save_outcome: SaveOutcome::Attempted,
        readback_outcome: ReadbackOutcome::NotAttempted,
        ..lifecycle_verified_publication()?
    };
    must_fail(
        &classify_publication(&record).map(|_| ()),
        "publication inferred from setup without readback",
    )?;
    let interrupted_record = PublicationV1 {
        save_step_outcome: Some("cancelled".to_owned()),
        save_outcome: SaveOutcome::Interrupted,
        observed_entry: None,
        readback_outcome: ReadbackOutcome::NotAttempted,
        readback_archive_digest: None,
        readback_manifest_digest: None,
        actual_stored_bytes: MeasuredU64 {
            value: None,
            unknown_reason: Some(ObservationGapV1 {
                code: ObservationGapCode::IncompleteAttribution,
                detail: "gc1-qualification-lifecycle-cancelled-bytes".to_owned(),
                affected_recipe_ids: Vec::new(),
                evidence_ref: "actions-cache-readback".to_owned(),
            }),
        },
        ..lifecycle_verified_publication()?
    };
    if classify_publication(&interrupted_record)? != PublicationOutcome::Interrupted {
        return Err(invalid("cancelled upload escaped its interruption"));
    }
    Ok(())
}

/// C11: cancelled cleanup keeps the verified replacement; predecessor charges
/// stay retained and the completed remote entry reconciles before admission.
pub(crate) fn check_cleanup_cancel_keeps_replacement() -> Result<(), Box<dyn Error>> {
    let verified = lifecycle_verified_publication()?;
    if classify_publication(&verified)? != PublicationOutcome::Verified {
        return Err(invalid("verified replacement lost its verdict"));
    }
    let bytes = lifecycle_frozen_bytes();
    let entry = verified
        .observed_entry
        .as_ref()
        .ok_or_else(|| invalid("verified replacement lost its entry"))?;
    let readback = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &verified.frozen_archive_digest,
        frozen_archive_bytes: entry.size_in_bytes,
        restored_bytes: Some(&bytes),
        inventory_complete: true,
        observed_entry: Some(entry),
        expected_key: &verified.generation.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(readback, ReadbackVerdict::Verified { .. }) {
        return Err(invalid("completed remote entry failed reconciliation"));
    }
    require_empty_restore_target(false)?;
    must_fail(
        &require_empty_restore_target(true),
        "occupied restore target admitted",
    )?;
    let retained = retain_usable_families(
        &[
            verified.generation.cache_key(),
            lifecycle_generation(b"gc1-qualification-lifecycle-retired")?.cache_key(),
        ],
        &lifecycle_generation(b"gc1-qualification-lifecycle-retired")?.cache_key(),
    )?;
    if retained != vec![verified.generation.cache_key()] {
        return Err(invalid("cleanup dropped the usable replacement"));
    }
    let next_save = lifecycle_save(true, None, true, false, SaveHalt::Running);
    if decide_save(&next_save)? != SaveOutcome::Attempted {
        return Err(invalid("reconciled replacement blocked admission"));
    }
    Ok(())
}

/// C11: cancelled runs record interrupted observations instead of zeros, and
/// the last usable predecessor stays available.
#[expect(
    clippy::too_many_lines,
    reason = "cancellation case keeps three observed phases together"
)]
pub(crate) fn check_cancelled_observations_stay_interrupted() -> Result<(), Box<dyn Error>> {
    let mut session = ObservationSession::begin(
        "gc1-qualification-c11",
        "cancel-preparation",
        &new_session_id(),
        &digest_b3(b"gc1-qualification-lifecycle-cancel-context"),
    )
    .map_err(|error| invalid(&error.to_string()))?;
    let compiler = session
        .compiler_evidence(
            Some(1),
            Some(0),
            Some(0),
            Some(0),
            Some(2),
            "ledger-drained cancelled compiler events",
        )
        .map_err(|error| invalid(&error.to_string()))?;
    session
        .provide_compiler(compiler)
        .map_err(|error| invalid(&error.to_string()))?;
    let native = session
        .native_evidence(Some(0), Some(0), Some(0), Some(1), "cancelled native calls")
        .map_err(|error| invalid(&error.to_string()))?;
    session
        .provide_native(native)
        .map_err(|error| invalid(&error.to_string()))?;
    let origin = session
        .origin_evidence(Some(3), Some(1_048_576), "cancelled origin acquisition")
        .map_err(|error| invalid(&error.to_string()))?;
    session
        .provide_origin(origin)
        .map_err(|error| invalid(&error.to_string()))?;
    let cache = session
        .cache_evidence(Some(512), "cancelled cache bytes")
        .map_err(|error| invalid(&error.to_string()))?;
    session
        .provide_cache(cache)
        .map_err(|error| invalid(&error.to_string()))?;
    let report = session
        .finish(ChildOutcome::Failure)
        .map_err(|error| invalid(&error.to_string()))?;
    if report.compiler_interrupted_calls.value != Some(2) {
        return Err(invalid("cancelled run reported zero interruptions"));
    }
    if !report.gaps.is_empty() {
        return Err(invalid("fully observed cancellation reported gaps"));
    }
    if !report.child_failed {
        return Err(invalid("cancelled run reported success"));
    }
    let mut unknown = ObservationSession::begin(
        "gc1-qualification-c11",
        "cancel-unknown",
        &new_session_id(),
        &digest_b3(b"gc1-qualification-lifecycle-cancel-context"),
    )
    .map_err(|error| invalid(&error.to_string()))?;
    let unknown_compiler = unknown
        .compiler_evidence(None, None, None, None, None, "cancelled before attribution")
        .map_err(|error| invalid(&error.to_string()))?;
    unknown
        .provide_compiler(unknown_compiler)
        .map_err(|error| invalid(&error.to_string()))?;
    let unknown_native = unknown
        .native_evidence(
            None,
            None,
            None,
            None,
            "cancelled before native attribution",
        )
        .map_err(|error| invalid(&error.to_string()))?;
    unknown
        .provide_native(unknown_native)
        .map_err(|error| invalid(&error.to_string()))?;
    let unknown_origin = unknown
        .origin_evidence(None, None, "cancelled before origin attribution")
        .map_err(|error| invalid(&error.to_string()))?;
    unknown
        .provide_origin(unknown_origin)
        .map_err(|error| invalid(&error.to_string()))?;
    let unknown_cache = unknown
        .cache_evidence(None, "cancelled before cache attribution")
        .map_err(|error| invalid(&error.to_string()))?;
    unknown
        .provide_cache(unknown_cache)
        .map_err(|error| invalid(&error.to_string()))?;
    let unknown_report = unknown
        .finish(ChildOutcome::Failure)
        .map_err(|error| invalid(&error.to_string()))?;
    if unknown_report.gaps.is_empty() {
        return Err(invalid("unobserved cancellation reported no gaps"));
    }
    if unknown_report.dependency_compilations.value.is_some() {
        return Err(invalid("unobserved cancellation reported measured work"));
    }
    let current = lifecycle_generation(b"gc1-qualification-lifecycle-cancelled")?;
    let predecessor = lifecycle_generation(b"gc1-qualification-lifecycle-predecessor")?;
    let job = lifecycle_job("push", "refs/heads/main", 111, 1, 1112, "reader")?;
    let request = lifecycle_discovery(
        job,
        current,
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![lifecycle_entry(
            &predecessor,
            "refs/heads/main",
            1110,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision != DiscoveryDecision::PredecessorFallback {
        return Err(invalid("cancelled preparation dropped its predecessor"));
    }
    let visible: Vec<&SelectableEntry> = request.entries.iter().collect();
    let retained = select_predecessor(&request, &visible)
        .ok_or_else(|| invalid("predecessor selection lost its candidate"))?;
    if retained.entry.cache_id != 1110 {
        return Err(invalid("predecessor selection kept the wrong entry"));
    }
    Ok(())
}
