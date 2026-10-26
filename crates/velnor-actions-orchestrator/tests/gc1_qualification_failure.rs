// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! T61 publication and recovery qualification helpers.
//!
//! Included by `gc1_qualification_cases.rs` through the existing
//! `gc1_qualification` module; the `c13` through `c17` tests below exercise
//! skipped and failed producers, denied saves, damaged caches, budget
//! admission, and coverage growth through real owned interfaces with local
//! fixtures only.

use std::error::Error;

use velnor_actions_contract::canonical::{Digest, digest_b3, digest_b3_typed};
use velnor_actions_contract::workflow::cache_context::{
    JobBindingV1, MeasuredU64, ObservationGapCode, ObservationGapV1,
};
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};
use velnor_actions_mise::compiler_observer::{
    CompilerCompletion, CompilerEvent, CompilerInvocation, CompilerLedger, OPERATION_COMPILATION,
    ROLE_APPLICATION, ROLE_DEPENDENCY, TOOL_KIND_RUSTC,
};
use velnor_actions_orchestrator::cache_publication::{
    PUBLICATION_SCHEMA, PublicationOutcome, PublicationV1, ReadbackOutcome, SaveHalt, SaveOutcome,
    SaveRequest, classify_publication, decide_save,
};
use velnor_actions_orchestrator::cache_recovery::{
    ObservedEntry, ReadbackInputs, ReadbackVerdict, decide_readback, require_empty_restore_target,
    retain_usable_families, sha256_hex,
};
use velnor_actions_orchestrator::{
    AdmissionDecision, AdmissionRequest, BLOCKED_GENERATION_SLOTS_EXHAUSTED,
    BLOCKED_INSUFFICIENT_HEADROOM, BLOCKED_INSUFFICIENT_LOCAL_DISK,
    BLOCKED_UNKNOWN_FOREIGN_EXPOSURE, BLOCKED_UNKNOWN_INVENTORY, BLOCKED_UNKNOWN_LOCAL_DISK,
    BLOCKED_UNKNOWN_PENDING_WRITES, BLOCKED_UNKNOWN_RUNTIME_ENVELOPE, EntryTupleV1,
    HARD_CEILING_BYTES, MAX_GENERATION_SLOTS, OPERATING_CEILING_BYTES, OUTER_MEMBER_NAME,
    OuterEnvelope, OuterEnvelopeIdentity, PreflightMeasurement, UNKNOWN_HOSTED_PACKAGING,
    build_inventory, decide_admission, inventory_digest, measure_envelope, measured_bytes,
    unknown_bytes, unknown_envelope,
};

use super::cache_coverage_replay::{
    FrozenArchive, FrozenProofV1, MbxInitialState, PairedInputBinding, QualificationOutcome,
    freeze_archive, verify_replay_frozen,
};
use super::cache_discovery::{
    DiscoveryDecision, DiscoveryRequest, EntryProvenance, RestoreObservation, SelectableEntry,
    decide_discovery, select_exact, select_predecessor, verify_restore,
};
use super::cache_intent::{
    IntentDraft, IntentObservation, IntentRecheck, derive_intent, intent_authorizes_mutation,
    intent_canonical_bytes, intent_digest, parse_intent_bytes, recheck_intent,
};
use super::compiler_observation::{
    ChildOutcome, ObservationReport, ObservationSession, new_session_id,
};
use super::gc1_qualification_inputs::{
    APP_ID, LIB_ID, LINUX_TARGET, base_coverage, changed_closure_ledger, cold_ledger,
    coverage_variant, eligible_ids, require_changed_coverage, require_measured_zero,
    require_nonzero, require_same_coverage, require_valid_digests, require_zero,
    warm_native_ledger,
};
use super::must_fail;
use crate::gc1_qualification_support::{
    ObservationClosure, Workload, invalid, require_complete_observation, require_known_case_ids,
    require_nonempty_workload, require_qualification_destructive,
};
pub(crate) fn check_failure_harness(case_id: &str, detail: &str) -> Result<(), Box<dyn Error>> {
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

fn session_error(error: impl std::fmt::Display) -> Box<dyn Error> {
    invalid(&error.to_string())
}

pub(crate) fn failure_job(
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
        plan_digest: Digest::parse(&digest_b3(b"gc1-qualification-failure-plan"))?,
    };
    job.validate()?;
    Ok(job)
}

fn failure_generation_with_compat(
    coverage_seed: &[u8],
    compat_seed: &[u8],
    repair_revision: u64,
) -> Result<GenerationId, Box<dyn Error>> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(b"gc1-qualification-failure-group"))?,
        compatibility_digest: Digest::parse(&digest_b3(compat_seed))?,
        coverage_digest: Digest::parse(&digest_b3(coverage_seed))?,
        repair_revision,
    })
}

pub(crate) fn failure_generation(coverage_seed: &[u8]) -> Result<GenerationId, Box<dyn Error>> {
    failure_generation_with_compat(coverage_seed, b"gc1-qualification-failure-compat", 0)
}

fn failure_entry(
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

fn failure_discovery(
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
        inventory_digest: digest_b3_typed(b"gc1-qualification-failure-inventory"),
        inventory_complete,
        entries,
        active_coverage_ids: vec!["coverage-a".to_owned()],
    }
}

fn failure_frozen_bytes() -> Vec<u8> {
    let mut bytes = b"gc1-qualification-failure-candidate:".to_vec();
    for round in 0_u32..64 {
        bytes.extend_from_slice(&round.to_le_bytes());
    }
    bytes
}

fn failure_frozen() -> Result<FrozenArchive, Box<dyn Error>> {
    freeze_archive(&failure_frozen_bytes()).map_err(|error| invalid(&error.to_string()))
}

fn failure_proof(
    frozen: &FrozenArchive,
    coverage_seed: &[u8],
    job: JobBindingV1,
) -> Result<FrozenProofV1, Box<dyn Error>> {
    let generation = failure_generation(coverage_seed)?;
    let proof = FrozenProofV1 {
        schema: 1,
        producer_job: job,
        generation,
        manifest_digest: digest_b3_typed(b"gc1-qualification-failure-manifest"),
        frozen_archive_digest: frozen.archive_sha256.clone(),
        frozen_archive_bytes: frozen.archive_bytes,
        frozen_at: "2026-10-26T00:00:00Z".to_owned(),
        paired_input_archive_bindings: vec![
            PairedInputBinding {
                input_role: "sources".to_owned(),
                archive_digest: digest_b3_typed(b"gc1-qualification-failure-sources"),
            },
            PairedInputBinding {
                input_role: "tools".to_owned(),
                archive_digest: digest_b3_typed(b"gc1-qualification-failure-tools"),
            },
        ],
        restore_context_digest: digest_b3_typed(b"gc1-qualification-failure-restore-context"),
        restored_state_index_digest: digest_b3_typed(b"gc1-qualification-failure-state-index"),
        restored_projection_index_digest: digest_b3_typed(
            b"gc1-qualification-failure-projection-index",
        ),
        metadata_roundtrip_passed: true,
        ephemeral_mbx_initial_state: MbxInitialState::Empty,
        ephemeral_mbx_evidence: "ephemeral MBX store observed empty before replay".to_owned(),
        replay_recipe_ids: vec!["check".to_owned(), "clippy".to_owned(), "test".to_owned()],
        replay_command_ledger_digest: digest_b3_typed(b"gc1-qualification-failure-ledger"),
        replay_observation_digest: digest_b3_typed(b"gc1-qualification-failure-observation"),
        eligible_dependency_ids_digest: digest_b3_typed(b"gc1-qualification-failure-eligible"),
        eligible_dependency_count: 3,
        unchanged_dependency_compilations: Some(0),
        covered_origin_acquisitions: Some(0),
        observation_gaps: Vec::new(),
        required_outputs_present: true,
        archive_digest_after_replay: frozen.archive_sha256.clone(),
        completed_coverage_digest: digest_b3_typed(b"gc1-qualification-failure-completed"),
        qualification_outcome: QualificationOutcome::Pass,
    };
    verify_replay_frozen(&proof, frozen).map_err(|error| invalid(&error.to_string()))?;
    Ok(proof)
}

fn failure_save(
    admission_admitted: bool,
    denied_reason: Option<&str>,
    candidate_complete: bool,
    exact_usable_exists: bool,
    candidate_bytes: Option<u64>,
    ceiling_bytes: u64,
    save_halt: SaveHalt,
) -> SaveRequest {
    SaveRequest {
        admission_admitted,
        candidate_complete,
        exact_usable_exists,
        denied_reason: denied_reason.map(str::to_owned),
        candidate_bytes,
        ceiling_bytes,
        save_halt,
    }
}

fn failure_invocation(event_id: &str, package_id: &str, role: &str) -> CompilerInvocation {
    CompilerInvocation {
        event_id: event_id.to_owned(),
        recipe_id: "internal/lint/dev".to_owned(),
        package_id: package_id.to_owned(),
        dependency_role: role.to_owned(),
        lane_id: "build".to_owned(),
        tool_kind: TOOL_KIND_RUSTC.to_owned(),
        executable: "/tools/rust/prefix/bin/rustc".to_owned(),
        argv: vec![
            "rustc".to_owned(),
            "--crate-name".to_owned(),
            "failure".to_owned(),
            "--edition=2021".to_owned(),
            "src/lib.rs".to_owned(),
        ],
        wrapper_chain: Vec::new(),
        operation_kind: OPERATION_COMPILATION.to_owned(),
        doc_phase: None,
        cwd: "/repo".to_owned(),
    }
}

fn failure_success() -> CompilerCompletion {
    CompilerCompletion {
        exit_code: Some(0),
        signal: None,
        interrupted: false,
    }
}

fn failure_failed() -> CompilerCompletion {
    CompilerCompletion {
        exit_code: Some(1),
        signal: None,
        interrupted: false,
    }
}

fn failure_interrupted() -> CompilerCompletion {
    CompilerCompletion {
        exit_code: None,
        signal: None,
        interrupted: true,
    }
}

fn failure_ledger(
    attempt: &str,
    failed: bool,
    interrupted: bool,
) -> Result<CompilerLedger, Box<dyn Error>> {
    let mut ledger = CompilerLedger::new();
    for (index, package_id) in [LIB_ID, APP_ID].iter().enumerate() {
        let role = if *package_id == APP_ID {
            ROLE_APPLICATION
        } else {
            ROLE_DEPENDENCY
        };
        let event_id = format!("evt-{attempt}-{index}");
        ledger.record(CompilerEvent {
            invocation: failure_invocation(&event_id, package_id, role),
            completion: None,
        })?;
        let completion = if failed && index == 0 {
            failure_failed()
        } else if interrupted {
            failure_interrupted()
        } else {
            failure_success()
        };
        ledger.complete(&event_id, completion)?;
    }
    Ok(ledger)
}

/// C13: an intentionally skipped writer records no units, so it cannot erase
/// the required consumer checks; missing and duplicate unit ledgers are
/// rejected and the skipped writer manufactures no coverage.
pub(crate) fn check_skipped_writer_keeps_required() -> Result<(), Box<dyn Error>> {
    let skipped = CompilerLedger::new();
    require_zero(
        "skipped writer dependency compilations",
        skipped.dependency_compilations(),
    )?;
    if skipped.failed() != 0 {
        return Err(invalid("skipped writer recorded failures it never ran"));
    }
    must_fail(
        &require_nonzero(
            "skipped writer manufactured coverage",
            skipped.dependency_compilations(),
        ),
        "skipped writer manufactured completed coverage",
    )?;
    let mut once = CompilerLedger::new();
    once.record(CompilerEvent {
        invocation: failure_invocation("evt-skip-0", LIB_ID, ROLE_DEPENDENCY),
        completion: None,
    })?;
    must_fail(
        &once.record(CompilerEvent {
            invocation: failure_invocation("evt-skip-0", LIB_ID, ROLE_DEPENDENCY),
            completion: None,
        }),
        "duplicate unit ledger admitted",
    )?;
    must_fail(
        &once.clone().complete("evt-skip-missing", failure_success()),
        "missing unit receipt admitted",
    )?;
    let generation = failure_generation(b"gc1-qualification-failure-skipped")?;
    let job = failure_job("push", "refs/heads/main", 131, 1, 1311, "reader")?;
    let request = failure_discovery(
        job,
        generation,
        vec!["refs/heads/main"],
        "refs/heads/main",
        Vec::new(),
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision != DiscoveryDecision::ColdMissing {
        return Err(invalid("skipped writer erased the required cold path"));
    }
    let cold = cold_ledger()?;
    require_nonzero(
        "required cold dependency compilations",
        cold.dependency_compilations(),
    )?;
    if cold.failed() != 0 {
        return Err(invalid("required cold path recorded failures"));
    }
    Ok(())
}

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

fn failure_success_report() -> Result<ObservationReport, Box<dyn Error>> {
    let ledger = cold_ledger()?;
    require_nonzero(
        "candidate dependency compilations",
        ledger.dependency_compilations(),
    )?;
    let native = super::gc1_qualification_inputs::native_cold_ledger()?;
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

fn failure_denied_record() -> Result<PublicationV1, Box<dyn Error>> {
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

/// C14: removing only the local transport copy before readback classifies the
/// absent publication as unavailable, never as verified or corrupt.
pub(crate) fn check_denied_absent_never_claims_entry() -> Result<(), Box<dyn Error>> {
    let bytes = failure_frozen_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let generation = failure_generation(b"gc1-qualification-failure-denied")?;
    let entry = ObservedEntry {
        repository_id: 1,
        cache_id: 1411,
        ref_name: "refs/heads/main".to_owned(),
        key: generation.cache_key(),
        version: "v1".to_owned(),
        size_in_bytes: size,
        created_at: "2026-10-26T00:00:00Z".to_owned(),
    };
    let present = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: Some(&bytes),
        inventory_complete: true,
        observed_entry: Some(&entry),
        expected_key: &generation.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(present, ReadbackVerdict::Verified { .. }) {
        return Err(invalid("exact readback missed its verification"));
    }
    let transport_removed = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: None,
        inventory_complete: true,
        observed_entry: Some(&entry),
        expected_key: &generation.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(transport_removed, ReadbackVerdict::Unavailable { .. }) {
        return Err(invalid("removed transport copy claimed a verdict"));
    }
    let no_entry = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: None,
        inventory_complete: true,
        observed_entry: None,
        expected_key: &generation.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(no_entry, ReadbackVerdict::Unavailable { .. }) {
        return Err(invalid("absent publication claimed an existing entry"));
    }
    Ok(())
}

/// C14: the denied save keeps the usable predecessor, and ordinary check
/// success or failure stands independently of the denial.
pub(crate) fn check_denied_save_keeps_predecessor() -> Result<(), Box<dyn Error>> {
    let predecessor = failure_generation(b"gc1-qualification-failure-predecessor")?;
    let current = failure_generation(b"gc1-qualification-failure-denied")?;
    if predecessor.cache_key() == current.cache_key() {
        return Err(invalid("denied generation merged with its predecessor"));
    }
    let job = failure_job("push", "refs/heads/main", 141, 1, 1412, "reader")?;
    let request = failure_discovery(
        job,
        current,
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![failure_entry(
            &predecessor,
            "refs/heads/main",
            1410,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision != DiscoveryDecision::PredecessorFallback {
        return Err(invalid("denied save dropped its predecessor"));
    }
    let visible: Vec<&SelectableEntry> = request.entries.iter().collect();
    let retained = select_predecessor(&request, &visible)
        .ok_or_else(|| invalid("predecessor selection lost its candidate"))?;
    if retained.entry.cache_id != 1410 {
        return Err(invalid("predecessor selection kept the wrong entry"));
    }
    let report = failure_success_report()?;
    report.validate().map_err(session_error)?;
    if report.child_failed {
        return Err(invalid("denied save rewrote check success as failure"));
    }
    if report.dependency_compilations.value != Some(2) {
        return Err(invalid("denied save erased candidate compiler work"));
    }
    if report.native_compilations.value != Some(1) {
        return Err(invalid("denied save erased candidate native work"));
    }
    if report.origin_acquisitions.value != Some(2) {
        return Err(invalid("denied save erased candidate origin work"));
    }
    let failed_check = failure_ledger("check-failed", true, false)?;
    if failed_check.failed() == 0 {
        return Err(invalid("denied save hid an ordinary check failure"));
    }
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
        return Err(invalid("denied save escaped alongside check failure"));
    }
    Ok(())
}

/// C15: eviction, expiry, and deletion leave the entry absent; the absent
/// readback stays unavailable and never claims an existing entry, while the
/// degraded inventory reports unavailability instead of a false miss.
pub(crate) fn check_absent_states_stay_unavailable() -> Result<(), Box<dyn Error>> {
    let bytes = failure_frozen_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let generation = failure_generation(b"gc1-qualification-failure-evicted")?;
    for inventory_complete in [true, false] {
        let verdict = decide_readback(&ReadbackInputs {
            frozen_archive_digest: &digest,
            frozen_archive_bytes: size,
            restored_bytes: Some(&bytes),
            inventory_complete,
            observed_entry: None,
            expected_key: &generation.cache_key(),
            expected_ref: "refs/heads/main",
            expected_version: "v1",
        })?;
        if !matches!(verdict, ReadbackVerdict::Unavailable { .. }) {
            return Err(invalid("absent entry claimed a readback verdict"));
        }
    }
    let job = failure_job("push", "refs/heads/main", 151, 1, 1511, "reader")?;
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
        return Err(invalid("evicted entry hid its miss"));
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
        return Err(invalid("expired inventory reported a false miss"));
    }
    let cold = cold_ledger()?;
    require_nonzero(
        "absent-entry cold dependency compilations",
        cold.dependency_compilations(),
    )?;
    must_fail(
        &require_nonzero("absent entry as warm pass", 0),
        "failed restore reported as a warm zero-work pass",
    )?;
    Ok(())
}

/// C15: corrupt inner payloads are rejected before use across digest, size,
/// key, ref, and version mismatches; the failed restore runs required checks
/// through bounded cold recovery instead of a warm pass.
#[expect(
    clippy::too_many_lines,
    reason = "corruption matrix needs every mismatch in one case"
)]
pub(crate) fn check_corrupt_inner_rejected() -> Result<(), Box<dyn Error>> {
    let bytes = failure_frozen_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let generation = failure_generation(b"gc1-qualification-failure-corrupt")?;
    let entry = ObservedEntry {
        repository_id: 1,
        cache_id: 1512,
        ref_name: "refs/heads/main".to_owned(),
        key: generation.cache_key(),
        version: "v1".to_owned(),
        size_in_bytes: size,
        created_at: "2026-10-26T00:00:00Z".to_owned(),
    };
    let mut damaged = bytes.clone();
    let last = damaged.len() - 1;
    damaged[last] ^= 0xff;
    let corrupt = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: Some(&damaged),
        inventory_complete: true,
        observed_entry: Some(&entry),
        expected_key: &generation.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(corrupt, ReadbackVerdict::Corrupt { .. }) {
        return Err(invalid("corrupt inner payload admitted"));
    }
    let short = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: Some(&bytes[..bytes.len() - 1]),
        inventory_complete: true,
        observed_entry: Some(&entry),
        expected_key: &generation.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(short, ReadbackVerdict::Corrupt { .. }) {
        return Err(invalid("truncated inner payload admitted"));
    }
    let wrong_key = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: Some(&bytes),
        inventory_complete: true,
        observed_entry: Some(&entry),
        expected_key: &failure_generation(b"gc1-qualification-failure-other")?.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(wrong_key, ReadbackVerdict::Corrupt { .. }) {
        return Err(invalid("wrong-key restore admitted"));
    }
    let wrong_ref = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: Some(&bytes),
        inventory_complete: true,
        observed_entry: Some(&entry),
        expected_key: &generation.cache_key(),
        expected_ref: "refs/heads/feature",
        expected_version: "v1",
    })?;
    if !matches!(wrong_ref, ReadbackVerdict::Corrupt { .. }) {
        return Err(invalid("wrong-ref restore admitted"));
    }
    let observation = RestoreObservation {
        intended_key: generation.cache_key(),
        matched_key: generation.cache_key(),
        restored_compatibility: generation.compatibility_digest.clone(),
        expected_compatibility: failure_generation_with_compat(
            b"gc1-qualification-failure-corrupt",
            b"gc1-qualification-failure-other-compat",
            0,
        )?
        .compatibility_digest
        .clone(),
        restored_coverage: generation.coverage_digest.clone(),
        expected_coverage: generation.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-qualification-failure-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-qualification-failure-manifest"),
    };
    must_fail(
        &verify_restore(&observation),
        "incompatible corrupt state admitted",
    )?;
    let cold = cold_ledger()?;
    require_nonzero(
        "corrupt-entry cold dependency compilations",
        cold.dependency_compilations(),
    )?;
    if cold.failed() != 0 {
        return Err(invalid("corrupt-entry cold recovery recorded failures"));
    }
    Ok(())
}

/// C15: a missing or timed-out restore keeps the candidate charge and never
/// authorizes treating the entry as corrupt; a transfer retry observes the
/// same unavailable verdict deterministically.
pub(crate) fn check_restore_timeout_needs_retry() -> Result<(), Box<dyn Error>> {
    let bytes = failure_frozen_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let generation = failure_generation(b"gc1-qualification-failure-timeout")?;
    let entry = ObservedEntry {
        repository_id: 1,
        cache_id: 1513,
        ref_name: "refs/heads/main".to_owned(),
        key: generation.cache_key(),
        version: "v1".to_owned(),
        size_in_bytes: size,
        created_at: "2026-10-26T00:00:00Z".to_owned(),
    };
    let inputs = ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: None,
        inventory_complete: true,
        observed_entry: Some(&entry),
        expected_key: &generation.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    };
    let first = decide_readback(&inputs)?;
    if !matches!(first, ReadbackVerdict::Unavailable { .. }) {
        return Err(invalid("timed-out restore claimed a verdict"));
    }
    let retry = decide_readback(&inputs)?;
    if retry != first {
        return Err(invalid("transfer retry changed the unavailable verdict"));
    }
    if matches!(first, ReadbackVerdict::Corrupt { .. }) {
        return Err(invalid("timeout authorized a corruption charge"));
    }
    require_empty_restore_target(false)?;
    must_fail(
        &require_empty_restore_target(true),
        "occupied restore target admitted",
    )?;
    Ok(())
}

/// C15: with usable predecessor A and reproducibly invalid candidate B in the
/// second slot, the corrupt B is rejected, the exact B entry is deleted under
/// reviewed repair intent while A is retained, and B-r1 is admitted only
/// after B is confirmed gone.
#[expect(
    clippy::too_many_lines,
    reason = "two-slot repair covers discovery, damage, and recheck"
)]
pub(crate) fn check_two_slot_repair_deletes_exact_b() -> Result<(), Box<dyn Error>> {
    let compat = b"gc1-qualification-failure-repair-compat";
    let predecessor =
        failure_generation_with_compat(b"gc1-qualification-failure-slot-a", compat, 0)?;
    let candidate = failure_generation_with_compat(b"gc1-qualification-failure-slot-b", compat, 0)?;
    if predecessor.cache_key() == candidate.cache_key() {
        return Err(invalid("repair slots merged into one generation"));
    }
    let job = failure_job("push", "refs/heads/main", 152, 1, 1521, "reader")?;
    let request = failure_discovery(
        job.clone(),
        candidate.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![
            failure_entry(
                &predecessor,
                "refs/heads/main",
                1520,
                EntryProvenance::Trusted,
            ),
            failure_entry(
                &candidate,
                "refs/heads/main",
                1521,
                EntryProvenance::Trusted,
            ),
        ],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("two-slot discovery missed the candidate"));
    }
    let bytes = failure_frozen_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let damaged = vec![0u8; bytes.len()];
    let corrupt = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: Some(&damaged),
        inventory_complete: true,
        observed_entry: Some(&ObservedEntry {
            repository_id: 1,
            cache_id: 1521,
            ref_name: "refs/heads/main".to_owned(),
            key: candidate.cache_key(),
            version: "v1".to_owned(),
            size_in_bytes: size,
            created_at: "2026-10-26T00:00:00Z".to_owned(),
        }),
        expected_key: &candidate.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(corrupt, ReadbackVerdict::Corrupt { .. }) {
        return Err(invalid("invalid candidate admitted for use"));
    }
    require_qualification_destructive(
        &crate::gc1_qualification_support::FixtureCacheRef {
            cache_id: 1521,
            namespace: crate::gc1_qualification_support::QUALIFICATION_NAMESPACE.to_owned(),
        },
        &[1521],
    )?;
    must_fail(
        &require_qualification_destructive(
            &crate::gc1_qualification_support::FixtureCacheRef {
                cache_id: 1520,
                namespace: crate::gc1_qualification_support::QUALIFICATION_NAMESPACE.to_owned(),
            },
            &[1521],
        ),
        "unapproved predecessor deletion admitted",
    )?;
    let retained = retain_usable_families(
        &[predecessor.cache_key(), candidate.cache_key()],
        &candidate.cache_key(),
    )?;
    if retained != vec![predecessor.cache_key()] {
        return Err(invalid("repair dropped the usable predecessor"));
    }
    must_fail(
        &retain_usable_families(&[predecessor.cache_key()], "bogus-key").map(|_| ()),
        "malformed repair charge admitted",
    )?;
    let repaired = failure_generation_with_compat(b"gc1-qualification-failure-slot-b", compat, 1)?;
    if repaired.cache_key() == candidate.cache_key() {
        return Err(invalid("repair revision kept the corrupt key"));
    }
    if !repaired.cache_key().ends_with("-r1") {
        return Err(invalid("repair revision drifted from its bounded key"));
    }
    let after = failure_discovery(
        job,
        repaired.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![failure_entry(
            &predecessor,
            "refs/heads/main",
            1520,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let after_record = decide_discovery(&after).map_err(|error| invalid(&error.to_string()))?;
    if after_record.decision != DiscoveryDecision::PredecessorFallback {
        return Err(invalid("repair recheck dropped its predecessor"));
    }
    let visible: Vec<&SelectableEntry> = after.entries.iter().collect();
    let kept = select_predecessor(&after, &visible)
        .ok_or_else(|| invalid("repair recheck lost predecessor A"))?;
    if kept.entry.cache_id != 1520 {
        return Err(invalid("repair recheck kept the wrong entry"));
    }
    let replacement_save =
        failure_save(true, None, true, false, Some(1024), 8192, SaveHalt::Running);
    if decide_save(&replacement_save)? != SaveOutcome::Attempted {
        return Err(invalid("confirmed repair blocked replacement admission"));
    }
    Ok(())
}

/// C15: bounded lookup-only and full-restore observations record their own
/// `last_accessed_at` values; both validate as bounded entries without
/// assuming keepalive or recency neutrality.
pub(crate) fn check_lookup_and_restore_recency_observed() -> Result<(), Box<dyn Error>> {
    let generation = failure_generation(b"gc1-qualification-failure-recency")?;
    let lookup = EntryTupleV1 {
        repository_id: 1,
        cache_id: 1530,
        ref_name: "refs/heads/main".to_owned(),
        key: generation.cache_key(),
        version: "v1".to_owned(),
        size_in_bytes: 512,
        created_at: "2026-10-26T00:00:00Z".to_owned(),
        last_accessed_at: "2026-10-26T00:00:00Z".to_owned(),
    };
    let restored = EntryTupleV1 {
        last_accessed_at: "2026-10-26T00:05:00Z".to_owned(),
        ..lookup.clone()
    };
    lookup
        .validate()
        .map_err(|error| invalid(&error.to_string()))?;
    restored
        .validate()
        .map_err(|error| invalid(&error.to_string()))?;
    if lookup.cache_id != restored.cache_id || lookup.key != restored.key {
        return Err(invalid("recency observations lost their entry identity"));
    }
    if lookup.last_accessed_at == restored.last_accessed_at {
        return Err(invalid("full restore assumed lookup recency"));
    }
    let inventory = build_inventory(1, "2026-10-26T00:05:00Z", vec![restored], 1)
        .map_err(|error| invalid(&error.to_string()))?;
    if inventory.entries.len() != 1 {
        return Err(invalid("bounded restore observation lost its entry"));
    }
    if inventory_digest(&inventory).map_err(|error| invalid(&error.to_string()))?
        != inventory_digest(&inventory).map_err(|error| invalid(&error.to_string()))?
    {
        return Err(invalid("restore inventory digest drifted"));
    }
    Ok(())
}

fn failure_envelope(
    inner_bytes: &[u8],
    measured_outer: u64,
    repack_allowance: u64,
) -> Result<OuterEnvelope, Box<dyn Error>> {
    use velnor_actions_orchestrator::FrozenInnerPayload;
    let inner = FrozenInnerPayload {
        archive_sha256: sha256_hex(inner_bytes),
        archive_bytes: u64::try_from(inner_bytes.len())
            .map_err(|_| invalid("fixture length out of range"))?,
    };
    let preflight = PreflightMeasurement {
        measured_outer_bytes: measured_outer,
        outer_sha256: sha256_hex(b"gc1-qualification-failure-outer"),
    };
    let identity = OuterEnvelopeIdentity {
        tool_identity: "gc1-qualification-failure-packer".to_owned(),
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
fn failure_admission(
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
        active_intent_digest: digest_b3_typed(b"gc1-qualification-failure-intent"),
        inventory_digest: digest_b3_typed(b"gc1-qualification-failure-inventory"),
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

/// C16: the full admission inequality admits an exact fit against the
/// operating ceiling and blocks a one-byte-over candidate; the stored sum and
/// the qualified outer envelope are both counted, and an observed maximum is
/// never accepted as proof of the repack bound.
#[expect(
    clippy::too_many_lines,
    reason = "admission case needs fit, overflow, save, and repack"
)]
pub(crate) fn check_admission_exact_fit_and_overflow() -> Result<(), Box<dyn Error>> {
    if HARD_CEILING_BYTES != 10_000_000_000 {
        return Err(invalid("hard ceiling drifted from its policy value"));
    }
    if OPERATING_CEILING_BYTES != 8_000_000_000 {
        return Err(invalid("operating ceiling drifted from its policy value"));
    }
    if MAX_GENERATION_SLOTS != 2 {
        return Err(invalid(
            "generation slot bound drifted from its policy value",
        ));
    }
    let bytes = failure_frozen_bytes();
    let inner = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let allowance = 100_000_000_u64;
    let measured_outer = 500_000_000_u64;
    let bound = measured_outer + allowance;
    let inventory_sum = OPERATING_CEILING_BYTES - bound;
    let generation = failure_generation(b"gc1-qualification-failure-budget")?;
    let inventory = build_inventory(
        1,
        "2026-10-26T00:00:00Z",
        vec![EntryTupleV1 {
            repository_id: 1,
            cache_id: 1610,
            ref_name: "refs/heads/main".to_owned(),
            key: generation.cache_key(),
            version: "v1".to_owned(),
            size_in_bytes: inventory_sum,
            created_at: "2026-10-26T00:00:00Z".to_owned(),
            last_accessed_at: "2026-10-26T00:00:00Z".to_owned(),
        }],
        1,
    )
    .map_err(|error| invalid(&error.to_string()))?;
    if inventory.sum_stored_bytes.value != Some(inventory_sum) {
        return Err(invalid("admission lost its stored-byte count"));
    }
    let job = failure_job("push", "refs/heads/main", 161, 1, 1611, "writer")?;
    let fit = decide_admission(&failure_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        inventory_sum,
        0,
        0,
        inner,
        failure_envelope(&bytes, measured_outer, allowance)?,
        1,
    ))
    .map_err(|error| invalid(&error.to_string()))?;
    if fit.decision != AdmissionDecision::Admitted {
        return Err(invalid("exact fit blocked admission"));
    }
    if fit.candidate_upper_bound_bytes.value != Some(bound) {
        return Err(invalid("admission lost its qualified upper bound"));
    }
    if fit.qualified_repack_allowance_bytes != allowance {
        return Err(invalid("admission lost its repack allowance"));
    }
    let over = decide_admission(&failure_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        inventory_sum + 1,
        0,
        0,
        inner,
        failure_envelope(&bytes, measured_outer, allowance)?,
        1,
    ))
    .map_err(|error| invalid(&error.to_string()))?;
    if over.decision != AdmissionDecision::Blocked {
        return Err(invalid("one-byte-over candidate admitted"));
    }
    if !over
        .blocked_reasons
        .contains(&BLOCKED_INSUFFICIENT_HEADROOM.to_owned())
    {
        return Err(invalid("overflow missed its headroom reason"));
    }
    let oversized_save = failure_save(
        true,
        None,
        true,
        false,
        Some(OPERATING_CEILING_BYTES + 1),
        OPERATING_CEILING_BYTES,
        SaveHalt::Running,
    );
    if decide_save(&oversized_save)? != SaveOutcome::Oversized {
        return Err(invalid("oversized publication escaped its bound"));
    }
    let exact_save = failure_save(
        true,
        None,
        true,
        false,
        Some(OPERATING_CEILING_BYTES),
        OPERATING_CEILING_BYTES,
        SaveHalt::Running,
    );
    if decide_save(&exact_save)? != SaveOutcome::Attempted {
        return Err(invalid("exact-fit save blocked admission"));
    }
    let mut repack_guess = measure_envelope(
        &velnor_actions_orchestrator::FrozenInnerPayload {
            archive_sha256: sha256_hex(&bytes),
            archive_bytes: inner,
        },
        &PreflightMeasurement {
            measured_outer_bytes: measured_outer,
            outer_sha256: sha256_hex(b"gc1-qualification-failure-outer"),
        },
        allowance,
        &OuterEnvelopeIdentity {
            tool_identity: "gc1-qualification-failure-packer".to_owned(),
            transport_member: OUTER_MEMBER_NAME.to_owned(),
        },
        true,
    )
    .map_err(|error| invalid(&error.to_string()))?;
    repack_guess.candidate_upper_bound_bytes = bound + 1;
    must_fail(
        &repack_guess.validate(),
        "observed maximum proved the repack bound",
    )?;
    Ok(())
}

/// C16: uncertain inventory, pending writes, foreign exposure, runtime
/// envelope, and local disk each block publication instead of admitting an
/// unbounded write.
#[expect(
    clippy::too_many_lines,
    reason = "uncertainty matrix needs every blocked charge"
)]
pub(crate) fn check_admission_blocks_uncertain() -> Result<(), Box<dyn Error>> {
    let bytes = failure_frozen_bytes();
    let inner = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let generation = failure_generation(b"gc1-qualification-failure-budget")?;
    let job = failure_job("push", "refs/heads/main", 161, 1, 1612, "writer")?;
    let envelope = failure_envelope(&bytes, 500_000_000, 100_000_000)?;
    let mut uncertain = failure_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        envelope,
        1,
    );
    uncertain.inventory_bytes = unknown_bytes("gc1-qualification-failure-inventory")
        .map_err(|error| invalid(&error.to_string()))?;
    let blocked_inventory =
        decide_admission(&uncertain).map_err(|error| invalid(&error.to_string()))?;
    if blocked_inventory.decision != AdmissionDecision::Blocked {
        return Err(invalid("unknown inventory admitted"));
    }
    if !blocked_inventory
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_INVENTORY.to_owned())
    {
        return Err(invalid("unknown inventory missed its reason"));
    }
    let mut pending = failure_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        failure_envelope(&bytes, 500_000_000, 100_000_000)?,
        1,
    );
    pending.pending_owned_bytes = unknown_bytes("gc1-qualification-failure-pending")
        .map_err(|error| invalid(&error.to_string()))?;
    let blocked_pending =
        decide_admission(&pending).map_err(|error| invalid(&error.to_string()))?;
    if !blocked_pending
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_PENDING_WRITES.to_owned())
    {
        return Err(invalid("unknown pending writes missed their reason"));
    }
    let mut foreign = failure_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        failure_envelope(&bytes, 500_000_000, 100_000_000)?,
        1,
    );
    foreign.foreign_inflight_bytes = unknown_bytes("gc1-qualification-failure-foreign")
        .map_err(|error| invalid(&error.to_string()))?;
    let blocked_foreign =
        decide_admission(&foreign).map_err(|error| invalid(&error.to_string()))?;
    if !blocked_foreign
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_FOREIGN_EXPOSURE.to_owned())
    {
        return Err(invalid("unknown foreign exposure missed its reason"));
    }
    let unknown = unknown_envelope(
        UNKNOWN_HOSTED_PACKAGING,
        "gc1-qualification-failure-packaging",
    )
    .map_err(|error| invalid(&error.to_string()))?;
    let unmeasured = failure_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        OuterEnvelope::Unknown(unknown),
        1,
    );
    let blocked_envelope =
        decide_admission(&unmeasured).map_err(|error| invalid(&error.to_string()))?;
    if blocked_envelope.decision != AdmissionDecision::Blocked {
        return Err(invalid("unknown runtime envelope admitted"));
    }
    if !blocked_envelope
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_RUNTIME_ENVELOPE.to_owned())
    {
        return Err(invalid("unknown runtime envelope missed its reason"));
    }
    let mut disk = failure_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        failure_envelope(&bytes, 500_000_000, 100_000_000)?,
        1,
    );
    disk.available_local_disk = unknown_bytes("gc1-qualification-failure-disk")
        .map_err(|error| invalid(&error.to_string()))?;
    let blocked_disk = decide_admission(&disk).map_err(|error| invalid(&error.to_string()))?;
    if !blocked_disk
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_LOCAL_DISK.to_owned())
    {
        return Err(invalid("unknown local disk missed its reason"));
    }
    let mut small_disk = failure_admission(
        job,
        generation,
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        failure_envelope(&bytes, 500_000_000, 100_000_000)?,
        1,
    );
    small_disk.available_local_disk = measured_bytes(10);
    small_disk.peak_local_disk_bound = measured_bytes(100);
    let blocked_small =
        decide_admission(&small_disk).map_err(|error| invalid(&error.to_string()))?;
    if blocked_small.decision != AdmissionDecision::Blocked {
        return Err(invalid("insufficient local disk admitted"));
    }
    if !blocked_small
        .blocked_reasons
        .contains(&BLOCKED_INSUFFICIENT_LOCAL_DISK.to_owned())
    {
        return Err(invalid("insufficient local disk missed its reason"));
    }
    Ok(())
}

/// C16: two occupied generation slots exhaust the repair budget until the
/// exact corrupt entry is deleted; planned deletion is never credited and
/// active coverage is never deleted to manufacture space.
#[expect(
    clippy::too_many_lines,
    reason = "slot repair covers exhaustion, credit, and admission"
)]
pub(crate) fn check_admission_slots_and_repair() -> Result<(), Box<dyn Error>> {
    let bytes = failure_frozen_bytes();
    let inner = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let generation_a = failure_generation(b"gc1-qualification-failure-slot-a")?;
    let generation_b = failure_generation(b"gc1-qualification-failure-slot-b")?;
    let job = failure_job("push", "refs/heads/main", 162, 1, 1621, "writer")?;
    let full = failure_admission(
        job.clone(),
        generation_b.clone(),
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        failure_envelope(&bytes, 500_000_000, 100_000_000)?,
        MAX_GENERATION_SLOTS,
    );
    let blocked_slots = decide_admission(&full).map_err(|error| invalid(&error.to_string()))?;
    if blocked_slots.decision != AdmissionDecision::Blocked {
        return Err(invalid("exhausted slots admitted a third generation"));
    }
    if !blocked_slots
        .blocked_reasons
        .contains(&BLOCKED_GENERATION_SLOTS_EXHAUSTED.to_owned())
    {
        return Err(invalid("exhausted slots missed their reason"));
    }
    let mut credited = failure_admission(
        job.clone(),
        generation_b.clone(),
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        failure_envelope(&bytes, 500_000_000, 100_000_000)?,
        1,
    );
    credited.claimed_freed_bytes = 1;
    must_fail(
        &decide_admission(&credited).map(|_| ()),
        "planned deletion credited before confirmation",
    )?;
    require_qualification_destructive(
        &crate::gc1_qualification_support::FixtureCacheRef {
            cache_id: 1621,
            namespace: crate::gc1_qualification_support::QUALIFICATION_NAMESPACE.to_owned(),
        },
        &[1621],
    )?;
    must_fail(
        &require_qualification_destructive(
            &crate::gc1_qualification_support::FixtureCacheRef {
                cache_id: 1620,
                namespace: crate::gc1_qualification_support::QUALIFICATION_NAMESPACE.to_owned(),
            },
            &[1621],
        ),
        "active coverage deleted to manufacture space",
    )?;
    let retained = retain_usable_families(
        &[generation_a.cache_key(), generation_b.cache_key()],
        &generation_b.cache_key(),
    )?;
    if retained != vec![generation_a.cache_key()] {
        return Err(invalid("slot repair dropped the usable generation"));
    }
    let freed = failure_admission(
        job,
        generation_b,
        OPERATING_CEILING_BYTES,
        7_000_000_000,
        0,
        0,
        inner,
        failure_envelope(&bytes, 500_000_000, 100_000_000)?,
        1,
    );
    let admitted = decide_admission(&freed).map_err(|error| invalid(&error.to_string()))?;
    if admitted.decision != AdmissionDecision::Admitted {
        return Err(invalid("confirmed repair blocked slot admission"));
    }
    Ok(())
}

/// C16: the verified publication keeps the final stored size inside the
/// preflight envelope, and a pending write without readback never counts as
/// publication.
pub(crate) fn check_verified_bytes_stay_inside_envelope() -> Result<(), Box<dyn Error>> {
    let bytes = failure_frozen_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let generation = failure_generation(b"gc1-qualification-failure-budget")?;
    let job = failure_job("push", "refs/heads/main", 163, 1, 1631, "writer")?;
    let verified = PublicationV1 {
        schema: PUBLICATION_SCHEMA,
        job: job.clone(),
        generation: generation.clone(),
        frozen_archive_digest: digest.clone(),
        manifest_digest: digest_b3_typed(b"gc1-qualification-failure-manifest"),
        frozen_proof_digest: digest_b3_typed(b"gc1-qualification-failure-proof"),
        admission_digest: digest_b3_typed(b"gc1-qualification-failure-admission"),
        save_step_outcome: Some("success".to_owned()),
        save_outcome: SaveOutcome::Attempted,
        observed_entry: Some(ObservedEntry {
            repository_id: 1,
            cache_id: 1631,
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
    if classify_publication(&verified)? != PublicationOutcome::Verified {
        return Err(invalid("verified publication lost its verdict"));
    }
    let mut drifted = verified.clone();
    drifted.actual_stored_bytes = measured_bytes(size + 1);
    must_fail(
        &classify_publication(&drifted).map(|_| ()),
        "stored size escaped its preflight envelope",
    )?;
    let mut unknown = verified.clone();
    unknown.actual_stored_bytes = unknown_bytes("gc1-qualification-failure-stored")
        .map_err(|error| invalid(&error.to_string()))?;
    must_fail(
        &classify_publication(&unknown).map(|_| ()),
        "uncertain stored size admitted as verified",
    )?;
    let pending = PublicationV1 {
        save_step_outcome: Some("success".to_owned()),
        save_outcome: SaveOutcome::Attempted,
        observed_entry: verified.observed_entry.clone(),
        readback_outcome: ReadbackOutcome::NotAttempted,
        readback_archive_digest: None,
        readback_manifest_digest: None,
        actual_stored_bytes: measured_bytes(size),
        ..failure_denied_record()?
    };
    must_fail(
        &classify_publication(&pending).map(|_| ()),
        "pending write counted as publication",
    )?;
    Ok(())
}

fn failure_intent(head_sha: &str, coverage_ids: Vec<String>) -> IntentDraft {
    IntentDraft {
        repository_id: 1,
        default_ref: "refs/heads/main".to_owned(),
        observed_head_sha: head_sha.to_owned(),
        configuration_digest: digest_b3_typed(b"gc1-qualification-failure-config"),
        active_group_ids: vec![digest_b3(b"gc1-qualification-failure-group")],
        active_coverage_ids: coverage_ids,
        derived_at: "2026-10-26T00:00:00Z".to_owned(),
        derivation_evidence_digest: digest_b3_typed(b"gc1-qualification-failure-intent-evidence"),
    }
}

/// C17: coverage growth under unchanged compatibility keeps both old and new
/// required coverage valid while the compatible prior state restores exactly.
pub(crate) fn check_coverage_growth_keeps_prior() -> Result<(), Box<dyn Error>> {
    let base = base_coverage()?;
    let grown = coverage_variant("0.1.0", "0.2.0", Some("tokio"), LINUX_TARGET, None, 'f')?;
    require_valid_digests(&base)?;
    require_valid_digests(&grown)?;
    require_same_coverage(&base, &base.clone())?;
    require_changed_coverage(&base, &grown)?;
    let old_generation = failure_generation(b"gc1-qualification-failure-coverage-old")?;
    let new_generation = failure_generation(b"gc1-qualification-failure-coverage-new")?;
    if old_generation.compatibility_digest != new_generation.compatibility_digest {
        return Err(invalid("coverage growth moved compatibility"));
    }
    if old_generation.cache_key() == new_generation.cache_key() {
        return Err(invalid("coverage growth kept the old key"));
    }
    let job = failure_job("push", "refs/heads/main", 171, 1, 1711, "reader")?;
    let request = failure_discovery(
        job,
        old_generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![failure_entry(
            &old_generation,
            "refs/heads/main",
            1710,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("compatible prior state lost its restore"));
    }
    let observation = RestoreObservation {
        intended_key: old_generation.cache_key(),
        matched_key: old_generation.cache_key(),
        restored_compatibility: old_generation.compatibility_digest.clone(),
        expected_compatibility: new_generation.compatibility_digest.clone(),
        restored_coverage: old_generation.coverage_digest.clone(),
        expected_coverage: old_generation.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-qualification-failure-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-qualification-failure-manifest"),
    };
    verify_restore(&observation).map_err(|error| invalid(&error.to_string()))?;
    Ok(())
}

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
