// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn session_error(error: impl std::fmt::Display) -> Box<dyn Error> {
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

pub(crate) fn failure_generation_with_compat(
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

pub(crate) fn failure_entry(
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

pub(crate) fn failure_discovery(
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

pub(crate) fn failure_frozen_bytes() -> Vec<u8> {
    let mut bytes = b"gc1-qualification-failure-candidate:".to_vec();
    for round in 0_u32..64 {
        bytes.extend_from_slice(&round.to_le_bytes());
    }
    bytes
}

pub(crate) fn failure_frozen() -> Result<FrozenArchive, Box<dyn Error>> {
    freeze_archive(&failure_frozen_bytes()).map_err(|error| invalid(&error.to_string()))
}

pub(crate) fn failure_proof(
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

pub(crate) fn failure_save(
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

pub(crate) fn failure_ledger(
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
