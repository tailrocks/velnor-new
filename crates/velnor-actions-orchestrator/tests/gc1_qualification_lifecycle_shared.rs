// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn check_lifecycle_harness(case_id: &str, detail: &str) -> Result<(), Box<dyn Error>> {
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

pub(crate) fn lifecycle_job(
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
        plan_digest: Digest::parse(&digest_b3(b"gc1-qualification-lifecycle-plan"))?,
    };
    job.validate()?;
    Ok(job)
}

pub(crate) fn lifecycle_generation(coverage_seed: &[u8]) -> Result<GenerationId, Box<dyn Error>> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(b"gc1-qualification-lifecycle-group"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"gc1-qualification-lifecycle-compat"))?,
        coverage_digest: Digest::parse(&digest_b3(coverage_seed))?,
        repair_revision: 0,
    })
}

pub(crate) fn lifecycle_entry(
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

pub(crate) fn lifecycle_discovery(
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
        inventory_digest: digest_b3_typed(b"gc1-qualification-lifecycle-inventory"),
        inventory_complete,
        entries,
        active_coverage_ids: vec!["coverage-a".to_owned()],
    }
}

pub(crate) fn lifecycle_frozen_bytes() -> Vec<u8> {
    let mut bytes = b"gc1-qualification-lifecycle-candidate:".to_vec();
    for round in 0_u32..64 {
        bytes.extend_from_slice(&round.to_le_bytes());
    }
    bytes
}

pub(crate) fn lifecycle_frozen() -> Result<FrozenArchive, Box<dyn Error>> {
    freeze_archive(&lifecycle_frozen_bytes()).map_err(|error| invalid(&error.to_string()))
}

pub(crate) fn lifecycle_proof(
    frozen: &FrozenArchive,
    coverage_seed: &[u8],
    job: JobBindingV1,
) -> Result<FrozenProofV1, Box<dyn Error>> {
    let generation = lifecycle_generation(coverage_seed)?;
    let proof = FrozenProofV1 {
        schema: 1,
        producer_job: job,
        generation,
        manifest_digest: digest_b3_typed(b"gc1-qualification-lifecycle-manifest"),
        frozen_archive_digest: frozen.archive_sha256.clone(),
        frozen_archive_bytes: frozen.archive_bytes,
        frozen_at: "2026-10-26T00:00:00Z".to_owned(),
        paired_input_archive_bindings: vec![
            PairedInputBinding {
                input_role: "sources".to_owned(),
                archive_digest: digest_b3_typed(b"gc1-qualification-lifecycle-sources"),
            },
            PairedInputBinding {
                input_role: "tools".to_owned(),
                archive_digest: digest_b3_typed(b"gc1-qualification-lifecycle-tools"),
            },
        ],
        restore_context_digest: digest_b3_typed(b"gc1-qualification-lifecycle-restore-context"),
        restored_state_index_digest: digest_b3_typed(b"gc1-qualification-lifecycle-state-index"),
        restored_projection_index_digest: digest_b3_typed(
            b"gc1-qualification-lifecycle-projection-index",
        ),
        metadata_roundtrip_passed: true,
        ephemeral_mbx_initial_state: MbxInitialState::Empty,
        ephemeral_mbx_evidence: "ephemeral MBX store observed empty before replay".to_owned(),
        replay_recipe_ids: vec!["check".to_owned(), "clippy".to_owned(), "test".to_owned()],
        replay_command_ledger_digest: digest_b3_typed(b"gc1-qualification-lifecycle-ledger"),
        replay_observation_digest: digest_b3_typed(b"gc1-qualification-lifecycle-observation"),
        eligible_dependency_ids_digest: digest_b3_typed(b"gc1-qualification-lifecycle-eligible"),
        eligible_dependency_count: 3,
        unchanged_dependency_compilations: Some(0),
        covered_origin_acquisitions: Some(0),
        observation_gaps: Vec::new(),
        required_outputs_present: true,
        archive_digest_after_replay: frozen.archive_sha256.clone(),
        completed_coverage_digest: digest_b3_typed(b"gc1-qualification-lifecycle-completed"),
        qualification_outcome: QualificationOutcome::Pass,
    };
    verify_replay_frozen(&proof, frozen).map_err(|error| invalid(&error.to_string()))?;
    Ok(proof)
}

pub(crate) fn lifecycle_save(
    admission_admitted: bool,
    denied_reason: Option<&str>,
    candidate_complete: bool,
    exact_usable_exists: bool,
    save_halt: SaveHalt,
) -> SaveRequest {
    SaveRequest {
        admission_admitted,
        candidate_complete,
        exact_usable_exists,
        denied_reason: denied_reason.map(str::to_owned),
        candidate_bytes: Some(1024),
        ceiling_bytes: 8192,
        save_halt,
    }
}

pub(crate) fn lifecycle_invocation(
    event_id: &str,
    package_id: &str,
    role: &str,
) -> CompilerInvocation {
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
            "lifecycle".to_owned(),
            "--edition=2021".to_owned(),
            "src/lib.rs".to_owned(),
        ],
        wrapper_chain: Vec::new(),
        operation_kind: OPERATION_COMPILATION.to_owned(),
        doc_phase: None,
        cwd: "/repo".to_owned(),
    }
}

pub(crate) fn lifecycle_success() -> CompilerCompletion {
    CompilerCompletion {
        exit_code: Some(0),
        signal: None,
        interrupted: false,
    }
}

pub(crate) fn lifecycle_failed() -> CompilerCompletion {
    CompilerCompletion {
        exit_code: Some(1),
        signal: None,
        interrupted: false,
    }
}

pub(crate) fn lifecycle_interrupted_completion() -> CompilerCompletion {
    CompilerCompletion {
        exit_code: None,
        signal: None,
        interrupted: true,
    }
}

/// C07: trust transitions across PR, main, preview, and stable release events.
pub(crate) fn check_event_trust_transitions() -> Result<(), Box<dyn Error>> {
    if trust_for_event(WorkflowEvent::Push) != Trust::Trusted {
        return Err(invalid("push lost its trusted writer authority"));
    }
    for event in [
        WorkflowEvent::PullRequest,
        WorkflowEvent::Fork,
        WorkflowEvent::MergeGroup,
        WorkflowEvent::Local,
        WorkflowEvent::Qualification,
    ] {
        if trust_for_event(event) != Trust::Pr {
            return Err(invalid("untrusted event escaped PR authority"));
        }
    }
    Ok(())
}
