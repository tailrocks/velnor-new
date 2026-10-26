// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// C13: when the reader completed the units, its skipped writer is expected;
/// the verified generation answers the recheck without repeating preparation.
pub(crate) fn check_completed_reader_keeps_skipped_writer() -> Result<(), Box<dyn Error>> {
    let reader = failure_ledger("reader-done", false, false)?;
    if reader.failed() != 0 {
        return Err(invalid("completed reader recorded failures"));
    }
    require_nonzero(
        "completed reader dependency compilations",
        reader.dependency_compilations(),
    )?;
    let generation = failure_generation(b"gc1-qualification-failure-skipped")?;
    let job = failure_job("push", "refs/heads/main", 131, 1, 1312, "reader")?;
    let request = failure_discovery(
        job,
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![failure_entry(
            &generation,
            "refs/heads/main",
            1310,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("completed reader lost its verified generation"));
    }
    let visible: Vec<&SelectableEntry> = request.entries.iter().collect();
    let candidate = select_exact(&request, &visible)
        .ok_or_else(|| invalid("completed reader lost its single candidate"))?;
    if candidate.entry.cache_id != 1310 {
        return Err(invalid("completed reader kept the wrong entry"));
    }
    let skipped_save = failure_save(true, None, true, true, Some(1024), 8192, SaveHalt::Running);
    if decide_save(&skipped_save)? != SaveOutcome::SkippedExisting {
        return Err(invalid("expected skipped writer repeated preparation"));
    }
    Ok(())
}

/// C13: an optional cache failure in a running worker continues through
/// permitted cold recovery and executes its units without claiming warm reuse.
pub(crate) fn check_optional_cache_error_recovers_cold() -> Result<(), Box<dyn Error>> {
    let generation = failure_generation(b"gc1-qualification-failure-optional")?;
    let job = failure_job("push", "refs/heads/main", 132, 1, 1321, "worker")?;
    let missing = failure_discovery(
        job.clone(),
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        Vec::new(),
        true,
    );
    let missing_record = decide_discovery(&missing).map_err(|error| invalid(&error.to_string()))?;
    if missing_record.decision != DiscoveryDecision::ColdMissing {
        return Err(invalid("optional cache fault skipped cold recovery"));
    }
    let degraded = failure_discovery(
        job,
        generation,
        vec!["refs/heads/main"],
        "refs/heads/main",
        Vec::new(),
        false,
    );
    let degraded_record =
        decide_discovery(&degraded).map_err(|error| invalid(&error.to_string()))?;
    if degraded_record.decision != DiscoveryDecision::ColdUnavailable {
        return Err(invalid("optional cache fault hid its unavailability"));
    }
    let recovered = failure_ledger("worker-retry", false, false)?;
    if recovered.failed() != 0 {
        return Err(invalid("cold recovery recorded failures"));
    }
    require_nonzero(
        "cold recovery dependency compilations",
        recovered.dependency_compilations(),
    )?;
    let warm = warm_native_ledger();
    require_measured_zero("recovered native work", warm.measured_native_compilations())?;
    must_fail(
        &require_nonzero("faulted worker warm pass", 0),
        "failed worker reported a warm zero-work pass",
    )?;
    Ok(())
}

/// C13: a hard failed or cancelled writer cannot make the already-finished
/// reader execute retroactively; Required fails with explicit missing and
/// failed units while an authorized rerun recovers with fresh evidence.
#[expect(
    clippy::too_many_lines,
    reason = "writer-failure case keeps ledgers and session together"
)]
pub(crate) fn check_hard_failed_writer_blocks_retroactive() -> Result<(), Box<dyn Error>> {
    let failed_writer = failure_ledger("writer-hard", true, false)?;
    if failed_writer.failed() == 0 {
        return Err(invalid("hard failed writer lost its failure"));
    }
    must_fail(
        &require_zero("hard failed writer failures", failed_writer.failed()),
        "hard failed writer passed as successful",
    )?;
    require_nonzero(
        "hard failed writer explicit failures",
        failed_writer.failed(),
    )?;
    if failed_writer.application_compilations() == 0 {
        return Err(invalid("failed writer lost its completed units"));
    }
    let cancelled_writer = failure_ledger("writer-cancel", false, true)?;
    if cancelled_writer.interrupted() == 0 {
        return Err(invalid("cancelled writer lost its interruption"));
    }
    must_fail(
        &require_zero(
            "cancelled writer interruptions",
            cancelled_writer.interrupted(),
        ),
        "cancelled writer passed as complete",
    )?;
    let finished_reader = failure_ledger("reader-finished", false, false)?;
    if finished_reader.failed() != 0 {
        return Err(invalid("finished reader inherited the writer failure"));
    }
    require_nonzero(
        "finished reader dependency compilations",
        finished_reader.dependency_compilations(),
    )?;
    let rerun = failure_ledger("writer-rerun", false, false)?;
    if rerun.failed() != 0 {
        return Err(invalid("authorized rerun kept the old failure"));
    }
    require_nonzero(
        "rerun dependency compilations",
        rerun.dependency_compilations(),
    )?;
    let mut session = ObservationSession::begin(
        "gc1-qualification-c13",
        "hard-failed-writer",
        &new_session_id(),
        &digest_b3(b"gc1-qualification-failure-writer-context"),
    )
    .map_err(session_error)?;
    let compiler = session
        .compiler_evidence(
            Some(1),
            Some(0),
            Some(0),
            Some(1),
            Some(1),
            "hard writer failure",
        )
        .map_err(session_error)?;
    session.provide_compiler(compiler).map_err(session_error)?;
    let native = session
        .native_evidence(
            Some(0),
            Some(0),
            Some(0),
            Some(1),
            "hard writer native calls",
        )
        .map_err(session_error)?;
    session.provide_native(native).map_err(session_error)?;
    let origin = session
        .origin_evidence(Some(1), Some(512), "hard writer origin acquisition")
        .map_err(session_error)?;
    session.provide_origin(origin).map_err(session_error)?;
    let cache = session
        .cache_evidence(Some(0), "no cache bytes from failed writer")
        .map_err(session_error)?;
    session.provide_cache(cache).map_err(session_error)?;
    let report = session
        .finish(ChildOutcome::Failure)
        .map_err(session_error)?;
    if !report.child_failed {
        return Err(invalid("hard failed writer reported success"));
    }
    if report.compiler_interrupted_calls.value != Some(1) {
        return Err(invalid("hard failed writer lost its interruption evidence"));
    }
    if !report.gaps.is_empty() {
        return Err(invalid("fully observed writer failure reported gaps"));
    }
    Ok(())
}

pub(crate) fn failure_success_report() -> Result<ObservationReport, Box<dyn Error>> {
    let ledger = cold_ledger()?;
    require_nonzero(
        "candidate dependency compilations",
        ledger.dependency_compilations(),
    )?;
    let native = super::super::gc1_qualification_inputs::native_cold_ledger()?;
    require_nonzero(
        "candidate native compilations",
        native.dependency_compilations(),
    )?;
    let mut session = ObservationSession::begin(
        "gc1-qualification-c14",
        "denied-save-candidate",
        &new_session_id(),
        &digest_b3(b"gc1-qualification-failure-candidate-context"),
    )
    .map_err(session_error)?;
    let compiler = session
        .compiler_evidence(
            Some(ledger.dependency_compilations()),
            Some(ledger.application_compilations()),
            Some(ledger.queries()),
            Some(ledger.failed()),
            Some(ledger.interrupted()),
            "ledger-drained denied-save candidate",
        )
        .map_err(session_error)?;
    session.provide_compiler(compiler).map_err(session_error)?;
    let observed = session
        .native_evidence(
            native.measured_native_compilations(),
            Some(native.queries()),
            Some(native.failed()),
            Some(native.interrupted()),
            "ledger-drained denied-save native calls",
        )
        .map_err(session_error)?;
    session.provide_native(observed).map_err(session_error)?;
    let origin = session
        .origin_evidence(Some(2), Some(1_048_576), "denied-save origin acquisition")
        .map_err(session_error)?;
    session.provide_origin(origin).map_err(session_error)?;
    let cache = session
        .cache_evidence(Some(0), "no cache bytes before denied save")
        .map_err(session_error)?;
    session.provide_cache(cache).map_err(session_error)?;
    session.finish(ChildOutcome::Success).map_err(session_error)
}

pub(crate) fn failure_denied_record() -> Result<PublicationV1, Box<dyn Error>> {
    let generation = failure_generation(b"gc1-qualification-failure-denied")?;
    let job = failure_job("push", "refs/heads/main", 141, 1, 1411, "writer")?;
    Ok(PublicationV1 {
        schema: PUBLICATION_SCHEMA,
        job,
        generation,
        frozen_archive_digest: sha256_hex(&failure_frozen_bytes()),
        manifest_digest: digest_b3_typed(b"gc1-qualification-failure-manifest"),
        frozen_proof_digest: digest_b3_typed(b"gc1-qualification-failure-proof"),
        admission_digest: digest_b3_typed(b"gc1-qualification-failure-admission"),
        save_step_outcome: Some("success".to_owned()),
        save_outcome: SaveOutcome::Denied,
        observed_entry: None,
        readback_outcome: ReadbackOutcome::NotAttempted,
        readback_archive_digest: None,
        readback_manifest_digest: None,
        actual_stored_bytes: MeasuredU64 {
            value: None,
            unknown_reason: Some(ObservationGapV1 {
                code: ObservationGapCode::IncompleteAttribution,
                detail: "gc1-qualification-failure-denied-bytes".to_owned(),
                affected_recipe_ids: Vec::new(),
                evidence_ref: "actions-cache-save".to_owned(),
            }),
        },
        active_intent_digest: digest_b3_typed(b"gc1-qualification-failure-intent"),
    })
}

/// C14: a denied save with a successful step records the denial with full
/// job identity while the absent key is never claimed as an existing entry.
pub(crate) fn check_denied_save_rejects_publication() -> Result<(), Box<dyn Error>> {
    let denied = failure_save(
        false,
        Some("cache write denied by read-only fixture"),
        true,
        false,
        Some(1024),
        8192,
        SaveHalt::Running,
    );
    if decide_save(&denied)? != SaveOutcome::Denied {
        return Err(invalid("denied save escaped its denial"));
    }
    must_fail(
        &decide_save(&failure_save(
            false,
            None,
            true,
            false,
            Some(1024),
            8192,
            SaveHalt::Running,
        ))
        .map(|_| ()),
        "denial without reason admitted",
    )?;
    let record = failure_denied_record()?;
    let entry_check = ObservedEntry {
        repository_id: 1,
        cache_id: 1411,
        ref_name: "refs/heads/main".to_owned(),
        key: record.generation.cache_key(),
        version: "v1".to_owned(),
        size_in_bytes: 512,
        created_at: "2026-10-26T00:00:00Z".to_owned(),
    };
    entry_check
        .validate()
        .map_err(|error| invalid(&error.to_string()))?;
    if entry_check.key != record.generation.cache_key() {
        return Err(invalid("denied publication lost its key identity"));
    }
    if classify_publication(&record)? != PublicationOutcome::Denied {
        return Err(invalid("successful step overrode the save denial"));
    }
    let generation = failure_generation(b"gc1-qualification-failure-denied")?;
    let job = failure_job("push", "refs/heads/main", 141, 1, 1411, "writer")?;
    let absent = failure_discovery(
        job,
        generation,
        vec!["refs/heads/main"],
        "refs/heads/main",
        Vec::new(),
        true,
    );
    let absent_record = decide_discovery(&absent).map_err(|error| invalid(&error.to_string()))?;
    if absent_record.decision != DiscoveryDecision::ColdMissing {
        return Err(invalid("denied key claimed an existing entry"));
    }
    let attempted = PublicationV1 {
        save_step_outcome: Some("success".to_owned()),
        save_outcome: SaveOutcome::Attempted,
        observed_entry: Some(entry_check),
        readback_outcome: ReadbackOutcome::NotAttempted,
        ..failure_denied_record()?
    };
    must_fail(
        &classify_publication(&attempted).map(|_| ()),
        "publication inferred from setup without readback",
    )?;
    Ok(())
}
