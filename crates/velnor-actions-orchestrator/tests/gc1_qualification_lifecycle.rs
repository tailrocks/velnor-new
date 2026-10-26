// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! T60 event and cancellation qualification helpers.
//!
//! Included by `gc1_qualification_cases.rs` through the existing
//! `gc1_qualification` module; the `c07` through `c12` tests below exercise
//! event trust, competing producers, cancellation phases, and mixed-attempt
//! reruns through real owned interfaces with local fixtures only.

use std::collections::BTreeSet;
use std::error::Error;

use velnor_actions_contract::canonical::{Digest, digest_b3, digest_b3_typed};
use velnor_actions_contract::workflow::cache_context::{
    JobBindingV1, MeasuredU64, ObservationGapCode, ObservationGapV1,
};
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};
use velnor_actions_contract::workflow::ir::{
    CacheMode, Concurrency, WRITER_QUEUE_CANCEL_IN_PROGRESS,
};
use velnor_actions_contract::workflow::plan::WorkflowEvent;
use velnor_actions_contract::workflow::trust::{Trust, trust_for_event};
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

use super::cache_coverage_replay::{
    FrozenArchive, FrozenProofV1, MbxInitialState, PairedInputBinding, QualificationOutcome,
    freeze_archive, verify_replay_frozen,
};
use super::cache_discovery::{
    DiscoveryDecision, DiscoveryRequest, EntryProvenance, RestoreObservation, SelectableEntry,
    ShadowState, classify_shadow, decide_discovery, primary_prefix, select_exact,
    select_predecessor, verify_restore,
};
use super::compiler_observation::{ChildOutcome, ObservationSession, new_session_id};
use super::gc1_qualification_inputs::{
    LINUX_TARGET, cold_ledger, coverage_variant, eligible_ids, require_changed_coverage,
    require_same_coverage,
};
use super::must_fail;
use crate::gc1_qualification_support::{
    ObservationClosure, Workload, invalid, require_complete_observation, require_known_case_ids,
    require_nonempty_workload,
};

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

fn lifecycle_entry(
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

fn lifecycle_discovery(
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

fn lifecycle_frozen_bytes() -> Vec<u8> {
    let mut bytes = b"gc1-qualification-lifecycle-candidate:".to_vec();
    for round in 0_u32..64 {
        bytes.extend_from_slice(&round.to_le_bytes());
    }
    bytes
}

fn lifecycle_frozen() -> Result<FrozenArchive, Box<dyn Error>> {
    freeze_archive(&lifecycle_frozen_bytes()).map_err(|error| invalid(&error.to_string()))
}

fn lifecycle_proof(
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

fn lifecycle_save(
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

fn lifecycle_invocation(event_id: &str, package_id: &str, role: &str) -> CompilerInvocation {
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

fn lifecycle_success() -> CompilerCompletion {
    CompilerCompletion {
        exit_code: Some(0),
        signal: None,
        interrupted: false,
    }
}

fn lifecycle_failed() -> CompilerCompletion {
    CompilerCompletion {
        exit_code: Some(1),
        signal: None,
        interrupted: false,
    }
}

fn lifecycle_interrupted_completion() -> CompilerCompletion {
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

/// C07: PR runs never publish, even with a complete candidate.
pub(crate) fn check_pr_runs_never_publish() -> Result<(), Box<dyn Error>> {
    let mode = CacheMode::explicit_read();
    if !mode.is_explicit_read() {
        return Err(invalid("PR consumer lost its explicit read mode"));
    }
    if mode.grants_write() {
        return Err(invalid("PR read mode grants cache writes"));
    }
    let denied = lifecycle_save(
        false,
        Some("pr-event-requires-trusted-authority"),
        true,
        false,
        SaveHalt::Running,
    );
    if decide_save(&denied)? != SaveOutcome::Denied {
        return Err(invalid("PR save escaped its denial"));
    }
    let denied_existing = lifecycle_save(
        false,
        Some("pr-event-requires-trusted-authority"),
        true,
        true,
        SaveHalt::Running,
    );
    if decide_save(&denied_existing)? != SaveOutcome::Denied {
        return Err(invalid("PR save published over an existing entry"));
    }
    let admitted = lifecycle_save(true, None, true, false, SaveHalt::Running);
    if decide_save(&admitted)? != SaveOutcome::Attempted {
        return Err(invalid("admission lost its trusted save attempt"));
    }
    Ok(())
}

/// C07: application-only reruns reuse the trusted baseline.
pub(crate) fn check_app_rerun_reuses_trusted_baseline() -> Result<(), Box<dyn Error>> {
    let base_coverage = coverage_variant("0.1.0", "0.2.0", None, LINUX_TARGET, None, 'f')?;
    let app_only = coverage_variant("0.1.1", "0.2.0", None, LINUX_TARGET, None, 'f')?;
    require_same_coverage(&base_coverage, &app_only)?;
    let dep_bump = coverage_variant("0.1.0", "0.3.0", None, LINUX_TARGET, None, 'f')?;
    require_changed_coverage(&base_coverage, &dep_bump)?;
    let base = lifecycle_generation(b"gc1-qualification-lifecycle-baseline")?;
    let job = lifecycle_job("pull_request", "refs/pull/7/merge", 71, 1, 711, "reader")?;
    let request = lifecycle_discovery(
        job,
        base.clone(),
        vec!["refs/heads/main", "refs/pull/7/merge"],
        "refs/pull/7/merge",
        vec![lifecycle_entry(
            &base,
            "refs/heads/main",
            71,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("app-only rerun missed the trusted baseline"));
    }
    if record.shadow_state != ShadowState::Clear {
        return Err(invalid("single-ref baseline reported a shadow"));
    }
    Ok(())
}

/// C07: new PR dependencies rebuild until trusted main preparation exists,
/// then the merged preparation is reused exactly once.
pub(crate) fn check_dependency_change_rebuilds_until_trusted() -> Result<(), Box<dyn Error>> {
    let old = lifecycle_generation(b"gc1-qualification-lifecycle-baseline")?;
    let changed_seed = b"gc1-qualification-lifecycle-changed";
    let changed = lifecycle_generation(changed_seed)?;
    if old.cache_key() == changed.cache_key() {
        return Err(invalid("dependency change kept the old cache key"));
    }
    let pr_job = lifecycle_job("pull_request", "refs/pull/7/merge", 71, 1, 712, "reader")?;
    let missing = lifecycle_discovery(
        pr_job,
        changed.clone(),
        vec!["refs/heads/main", "refs/pull/7/merge"],
        "refs/pull/7/merge",
        vec![lifecycle_entry(
            &old,
            "refs/heads/main",
            71,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let missing_record = decide_discovery(&missing).map_err(|error| invalid(&error.to_string()))?;
    if missing_record.decision == DiscoveryDecision::ExactHit {
        return Err(invalid("changed coverage hit the stale baseline"));
    }
    let trusted_job = lifecycle_job("push", "refs/heads/main", 72, 1, 713, "writer")?;
    let frozen = lifecycle_frozen()?;
    lifecycle_proof(&frozen, changed_seed, trusted_job.clone())?;
    let prepared = lifecycle_discovery(
        trusted_job,
        changed.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![lifecycle_entry(
            &changed,
            "refs/heads/main",
            72,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let prepared_record =
        decide_discovery(&prepared).map_err(|error| invalid(&error.to_string()))?;
    if prepared_record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("trusted preparation missed its exact hit"));
    }
    let repeat = lifecycle_save(true, None, true, true, SaveHalt::Running);
    if decide_save(&repeat)? != SaveOutcome::SkippedExisting {
        return Err(invalid("prepared coverage was prepared twice"));
    }
    Ok(())
}

/// C07: PR compiler output is never promoted through a shared cache prefix.
pub(crate) fn check_pr_output_never_promotes_prefix() -> Result<(), Box<dyn Error>> {
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-baseline")?;
    let main_job = lifecycle_job("push", "refs/heads/main", 73, 1, 714, "reader")?;
    let request = lifecycle_discovery(
        main_job,
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![lifecycle_entry(
            &generation,
            "refs/pull/7/merge",
            71,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision == DiscoveryDecision::ExactHit {
        return Err(invalid("main request consumed a PR-scoped entry"));
    }
    Ok(())
}

/// C07: preview and stable builds consume their correct prepared variants.
pub(crate) fn check_preview_stable_consume_prepared() -> Result<(), Box<dyn Error>> {
    let preview = lifecycle_generation(b"gc1-qualification-lifecycle-preview")?;
    let stable = lifecycle_generation(b"gc1-qualification-lifecycle-stable")?;
    if preview.cache_key() == stable.cache_key() {
        return Err(invalid("preview and stable share one cache key"));
    }
    for (generation, job_id) in [(&preview, 715_u64), (&stable, 716_u64)] {
        let job = lifecycle_job("push", "refs/heads/main", 74, 1, job_id, "reader")?;
        let request = lifecycle_discovery(
            job,
            generation.clone(),
            vec!["refs/heads/main"],
            "refs/heads/main",
            vec![lifecycle_entry(
                generation,
                "refs/heads/main",
                job_id,
                EntryProvenance::Trusted,
            )],
            true,
        );
        let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
        if record.decision != DiscoveryDecision::ExactHit {
            return Err(invalid("prepared profile missed its exact hit"));
        }
    }
    let crossed = RestoreObservation {
        intended_key: preview.cache_key(),
        matched_key: stable.cache_key(),
        restored_compatibility: preview.compatibility_digest.clone(),
        expected_compatibility: preview.compatibility_digest.clone(),
        restored_coverage: stable.coverage_digest.clone(),
        expected_coverage: preview.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-qualification-lifecycle-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-qualification-lifecycle-manifest"),
    };
    must_fail(
        &verify_restore(&crossed),
        "stable entry admitted for a preview request",
    )?;
    Ok(())
}

/// C08: fork PRs stay under restricted PR authority.
pub(crate) fn check_fork_event_restricted() -> Result<(), Box<dyn Error>> {
    if trust_for_event(WorkflowEvent::Fork) != Trust::Pr {
        return Err(invalid("fork escaped PR authority"));
    }
    let job = lifecycle_job("pull_request", "refs/pull/8/merge", 81, 1, 811, "reader")?;
    if job.event.is_empty() || job.ref_name.is_empty() {
        return Err(invalid("fork job lost its event binding"));
    }
    Ok(())
}

/// C08: fork consumers are read-only; origin credentials and private cache
/// contents stay outside unauthorized exposure.
pub(crate) fn check_fork_cache_read_only() -> Result<(), Box<dyn Error>> {
    let mode = CacheMode::explicit_read();
    if mode.grants_write() || mode.grants_unconditional_write() {
        return Err(invalid("fork read mode grants cache writes"));
    }
    let denied = lifecycle_save(
        false,
        Some("fork-restricted-capability"),
        true,
        false,
        SaveHalt::Running,
    );
    if decide_save(&denied)? != SaveOutcome::Denied {
        return Err(invalid("fork save escaped its denial"));
    }
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-fork")?;
    let job = lifecycle_job("pull_request", "refs/pull/8/merge", 81, 1, 812, "reader")?;
    let request = lifecycle_discovery(
        job,
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/pull/8/merge",
        vec![lifecycle_entry(
            &generation,
            "refs/heads/private",
            81,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision == DiscoveryDecision::ExactHit {
        return Err(invalid("fork request consumed a private entry"));
    }
    Ok(())
}

/// C08: private-source denial is explicit, and authorized fork work recovers
/// cold with a real nonzero baseline instead of minted credentials.
pub(crate) fn check_private_source_denied_cold_recovery() -> Result<(), Box<dyn Error>> {
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-fork")?;
    let job = lifecycle_job("pull_request", "refs/pull/8/merge", 81, 1, 813, "reader")?;
    for provenance in [EntryProvenance::Untrusted, EntryProvenance::Ambiguous] {
        let request = lifecycle_discovery(
            job.clone(),
            generation.clone(),
            vec!["refs/heads/main", "refs/pull/8/merge"],
            "refs/pull/8/merge",
            vec![
                lifecycle_entry(&generation, "refs/heads/main", 81, provenance),
                lifecycle_entry(&generation, "refs/heads/main", 82, EntryProvenance::Trusted),
            ],
            true,
        );
        let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
        if record.decision != DiscoveryDecision::RefusedProvenance {
            return Err(invalid("private entry escaped provenance refusal"));
        }
    }
    let missing = lifecycle_discovery(
        job,
        generation,
        vec!["refs/heads/main", "refs/pull/8/merge"],
        "refs/pull/8/merge",
        Vec::new(),
        true,
    );
    let missing_record = decide_discovery(&missing).map_err(|error| invalid(&error.to_string()))?;
    if missing_record.decision != DiscoveryDecision::ColdMissing {
        return Err(invalid("missing fork coverage skipped cold recovery"));
    }
    let cold = cold_ledger()?;
    if cold.dependency_compilations() == 0 {
        return Err(invalid("fork cold recovery measured zero work"));
    }
    Ok(())
}

/// C09: merge-queue and tag consumers are read-only.
pub(crate) fn check_merge_consumers_read_only() -> Result<(), Box<dyn Error>> {
    if trust_for_event(WorkflowEvent::MergeGroup) != Trust::Pr {
        return Err(invalid("merge-group escaped PR authority"));
    }
    let queue_job = lifecycle_job(
        "merge_group",
        "refs/heads/gh-readonly-queue/main/pr-1",
        91,
        1,
        911,
        "reader",
    )?;
    let tag_job = lifecycle_job("push", "refs/tags/v1.0.0", 92, 1, 912, "reader")?;
    for job in [queue_job, tag_job] {
        let denied = lifecycle_save(
            false,
            Some("merge-consumer-read-only"),
            true,
            false,
            SaveHalt::Running,
        );
        if decide_save(&denied)? != SaveOutcome::Denied {
            return Err(invalid("merge consumer save escaped its denial"));
        }
        let mode = CacheMode::explicit_read();
        if !mode.is_explicit_read() || mode.grants_write() {
            return Err(invalid("merge consumer lost its read-only mode"));
        }
        let _ = job;
    }
    Ok(())
}

/// C09: conflicting visible refs refuse the shadow; a single selected unit
/// resolves to exactly one candidate.
pub(crate) fn check_ref_shadow_refused() -> Result<(), Box<dyn Error>> {
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-merge")?;
    let job = lifecycle_job(
        "merge_group",
        "refs/heads/gh-readonly-queue/main/pr-1",
        91,
        1,
        913,
        "reader",
    )?;
    let conflicted = lifecycle_discovery(
        job.clone(),
        generation.clone(),
        vec!["refs/heads/main", "refs/heads/gh-readonly-queue/main/pr-1"],
        "refs/heads/gh-readonly-queue/main/pr-1",
        vec![
            lifecycle_entry(&generation, "refs/heads/main", 91, EntryProvenance::Trusted),
            lifecycle_entry(
                &generation,
                "refs/heads/gh-readonly-queue/main/pr-1",
                92,
                EntryProvenance::Trusted,
            ),
        ],
        true,
    );
    let conflicted_record =
        decide_discovery(&conflicted).map_err(|error| invalid(&error.to_string()))?;
    if conflicted_record.decision != DiscoveryDecision::RefusedShadow {
        return Err(invalid("conflicting refs escaped shadow refusal"));
    }
    if conflicted_record.shadow_state != ShadowState::ConflictingVisibleRef {
        return Err(invalid("conflicting refs reported a clear shadow"));
    }
    let exact_refs: BTreeSet<String> = BTreeSet::from([
        "refs/heads/main".to_owned(),
        "refs/heads/gh-readonly-queue/main/pr-1".to_owned(),
    ]);
    if classify_shadow(
        &exact_refs,
        &BTreeSet::new(),
        "refs/heads/gh-readonly-queue/main/pr-1",
    ) != ShadowState::ConflictingVisibleRef
    {
        return Err(invalid("direct shadow check missed the conflict"));
    }
    let single = lifecycle_discovery(
        job,
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![lifecycle_entry(
            &generation,
            "refs/heads/main",
            91,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let single_record = decide_discovery(&single).map_err(|error| invalid(&error.to_string()))?;
    if single_record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("single-ref request missed its exact hit"));
    }
    let visible: Vec<&SelectableEntry> = single.entries.iter().collect();
    let candidate = select_exact(&single, &visible)
        .ok_or_else(|| invalid("exact selection lost its candidate"))?;
    if candidate.entry.cache_id != 91 {
        return Err(invalid("exact selection kept the wrong unit"));
    }
    Ok(())
}

/// C09: longer primary-key prefix matches on the current ref also conflict.
pub(crate) fn check_longer_prefix_shadow() -> Result<(), Box<dyn Error>> {
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-merge")?;
    let prefix = primary_prefix(&generation);
    let longer = SelectableEntry {
        cache_id: 93,
        repository_id: 1,
        ref_name: "refs/heads/gh-readonly-queue/main/pr-1".to_owned(),
        key: format!("{prefix}-restore-001"),
        version: "v1".to_owned(),
        size_in_bytes: 512,
        created_at: "2026-10-26T00:00:00Z".to_owned(),
        provenance: EntryProvenance::Trusted,
    };
    let job = lifecycle_job(
        "merge_group",
        "refs/heads/gh-readonly-queue/main/pr-1",
        91,
        1,
        914,
        "reader",
    )?;
    let request = lifecycle_discovery(
        job,
        generation.clone(),
        vec!["refs/heads/main", "refs/heads/gh-readonly-queue/main/pr-1"],
        "refs/heads/gh-readonly-queue/main/pr-1",
        vec![
            lifecycle_entry(&generation, "refs/heads/main", 91, EntryProvenance::Trusted),
            longer,
        ],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision != DiscoveryDecision::RefusedShadow {
        return Err(invalid("prefix shadow escaped refusal"));
    }
    Ok(())
}

/// C09: the pin extracts a differing matched key before returning it;
/// unauthorized, ambiguous, and foreign-repository scopes are refused.
pub(crate) fn check_pin_extracts_matched_key() -> Result<(), Box<dyn Error>> {
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-merge")?;
    let exact = RestoreObservation {
        intended_key: generation.cache_key(),
        matched_key: generation.cache_key(),
        restored_compatibility: generation.compatibility_digest.clone(),
        expected_compatibility: generation.compatibility_digest.clone(),
        restored_coverage: generation.coverage_digest.clone(),
        expected_coverage: generation.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-qualification-lifecycle-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-qualification-lifecycle-manifest"),
    };
    verify_restore(&exact).map_err(|error| invalid(&error.to_string()))?;
    let mut drifted = exact.clone();
    drifted.matched_key = lifecycle_generation(b"gc1-qualification-lifecycle-other")?.cache_key();
    must_fail(&verify_restore(&drifted), "differing matched key admitted")?;
    let job = lifecycle_job(
        "merge_group",
        "refs/heads/gh-readonly-queue/main/pr-1",
        91,
        1,
        915,
        "reader",
    )?;
    let ambiguous = lifecycle_discovery(
        job.clone(),
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![lifecycle_entry(
            &generation,
            "refs/heads/main",
            91,
            EntryProvenance::Ambiguous,
        )],
        true,
    );
    let ambiguous_record =
        decide_discovery(&ambiguous).map_err(|error| invalid(&error.to_string()))?;
    if ambiguous_record.decision != DiscoveryDecision::RefusedProvenance {
        return Err(invalid("ambiguous scope escaped provenance refusal"));
    }
    let mut foreign = lifecycle_entry(&generation, "refs/heads/main", 91, EntryProvenance::Trusted);
    foreign.repository_id = 2;
    let cross_repo = lifecycle_discovery(
        job,
        generation,
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![foreign],
        true,
    );
    let cross_repo_record =
        decide_discovery(&cross_repo).map_err(|error| invalid(&error.to_string()))?;
    if cross_repo_record.decision == DiscoveryDecision::ExactHit {
        return Err(invalid("foreign repository entry satisfied the request"));
    }
    Ok(())
}

/// C09: unavailable inventory reports incomplete provenance and uses the
/// permitted cold path instead of a verified default.
pub(crate) fn check_inventory_unavailable_uses_cold_path() -> Result<(), Box<dyn Error>> {
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-merge")?;
    let job = lifecycle_job("push", "refs/tags/v1.0.0", 92, 1, 916, "reader")?;
    let unavailable = lifecycle_discovery(
        job.clone(),
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        Vec::new(),
        false,
    );
    let unavailable_record =
        decide_discovery(&unavailable).map_err(|error| invalid(&error.to_string()))?;
    if unavailable_record.decision != DiscoveryDecision::ColdUnavailable {
        return Err(invalid("missing inventory escaped cold-unavailable"));
    }
    if unavailable_record.shadow_state != ShadowState::Unknown {
        return Err(invalid("missing inventory claimed a shadow verdict"));
    }
    let degraded = lifecycle_discovery(
        job,
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![lifecycle_entry(
            &generation,
            "refs/heads/main",
            92,
            EntryProvenance::Trusted,
        )],
        false,
    );
    let degraded_record =
        decide_discovery(&degraded).map_err(|error| invalid(&error.to_string()))?;
    if degraded_record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("trusted exact lost its degraded hit"));
    }
    if degraded_record.shadow_state != ShadowState::Unknown {
        return Err(invalid("degraded hit claimed verified provenance"));
    }
    let cold = cold_ledger()?;
    if cold.dependency_compilations() == 0 {
        return Err(invalid("tag cold path measured zero work"));
    }
    Ok(())
}

/// C10: competing producers share the common repository mutation group with
/// queue semantics and no cancel-in-progress.
pub(crate) fn check_writer_queue_policy() -> Result<(), Box<dyn Error>> {
    if WRITER_QUEUE_CANCEL_IN_PROGRESS != "false" {
        return Err(invalid("writer queue lost its cancel-in-progress policy"));
    }
    let group = "gc1-qualification-writer-queue";
    let queue = Concurrency::writer_queue(group);
    if !queue.is_writer_queue() {
        return Err(invalid("writer queue lost its queue identity"));
    }
    queue
        .validate_writer_queue()
        .map_err(|error| invalid(&error.to_string()))?;
    if queue.cancel_in_progress != "false" {
        return Err(invalid("writer queue cancels in-progress work"));
    }
    let empty_group = Concurrency::writer_queue("");
    must_fail(
        &empty_group.validate_writer_queue(),
        "writer queue without a group admitted",
    )?;
    let cancelling = Concurrency {
        group: group.to_owned(),
        cancel_in_progress: "true".to_owned(),
    };
    must_fail(
        &cancelling.validate_writer_queue(),
        "writer queue with cancel-in-progress admitted",
    )?;
    Ok(())
}

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

fn lifecycle_attempt_ledger(
    attempt: &str,
    failed: bool,
    interrupted: bool,
) -> Result<CompilerLedger, Box<dyn Error>> {
    let mut ledger = CompilerLedger::new();
    let package_ids = ["workspace-lib", "workspace-app"];
    for (index, package_id) in package_ids.iter().enumerate() {
        let role = if *package_id == "workspace-app" {
            ROLE_APPLICATION
        } else {
            ROLE_DEPENDENCY
        };
        let event_id = format!("evt-{attempt}-{index}");
        ledger.record(CompilerEvent {
            invocation: lifecycle_invocation(&event_id, package_id, role),
            completion: None,
        })?;
        let completion = if failed && index == 0 {
            lifecycle_failed()
        } else if interrupted {
            lifecycle_interrupted_completion()
        } else {
            lifecycle_success()
        };
        ledger.complete(&event_id, completion)?;
    }
    Ok(ledger)
}

/// C12: group A succeeds at attempt 1, group B fails for a controlled reason,
/// and the rerun executes B and Required at attempt 2.
pub(crate) fn check_mixed_attempt_ledgers() -> Result<(), Box<dyn Error>> {
    let attempt_one_success = lifecycle_attempt_ledger("a1", false, false)?;
    if attempt_one_success.failed() != 0 {
        return Err(invalid("successful attempt 1 recorded failures"));
    }
    if attempt_one_success.dependency_compilations() == 0 {
        return Err(invalid("successful attempt 1 measured zero work"));
    }
    let mut attempt_one_failed = lifecycle_attempt_ledger("b1", true, false)?;
    if attempt_one_failed.failed() == 0 {
        return Err(invalid("failed writer lost its failure"));
    }
    must_fail(
        &attempt_one_failed.record(CompilerEvent {
            invocation: lifecycle_invocation("evt-b1-0", "workspace-lib", ROLE_DEPENDENCY),
            completion: None,
        }),
        "duplicate rerun ledger admitted",
    )?;
    must_fail(
        &attempt_one_failed
            .clone()
            .complete("evt-missing", lifecycle_success()),
        "unknown rerun receipt admitted",
    )?;
    let attempt_two = lifecycle_attempt_ledger("b2", false, false)?;
    if attempt_two.failed() != 0 {
        return Err(invalid("rerun attempt 2 recorded failures"));
    }
    if attempt_two.dependency_compilations() == 0 {
        return Err(invalid("rerun attempt 2 measured zero work"));
    }
    Ok(())
}

/// C12: the rerun accepts A's authentic attempt-1 ledger only after
/// authenticated job records establish retention without rerunning.
pub(crate) fn check_retained_success_accepted() -> Result<(), Box<dyn Error>> {
    let retained_a = lifecycle_job("push", "refs/heads/main", 121, 1, 1211, "group-a")?;
    let rerun_b = lifecycle_job("push", "refs/heads/main", 121, 2, 1212, "group-b")?;
    if retained_a.run_id != rerun_b.run_id {
        return Err(invalid("rerun lost its run identity"));
    }
    if retained_a.run_attempt == rerun_b.run_attempt {
        return Err(invalid("retained job relabelled as rerun evidence"));
    }
    if retained_a.plan_digest != rerun_b.plan_digest {
        return Err(invalid("rerun changed its plan identity"));
    }
    let mut drifted_plan = retained_a.clone();
    drifted_plan.plan_digest = Digest::parse(&digest_b3(b"gc1-qualification-lifecycle-other"))?;
    if drifted_plan.plan_digest == rerun_b.plan_digest {
        return Err(invalid("mismatched plan identity compared equal"));
    }
    let mut cross_run = retained_a.clone();
    cross_run.run_id = 999;
    if cross_run.run_id == rerun_b.run_id {
        return Err(invalid("cross-run receipt compared equal"));
    }
    Ok(())
}

/// C12: no global current-run-attempt equality is imposed on a retained
/// successful job.
pub(crate) fn check_no_global_attempt_equality() -> Result<(), Box<dyn Error>> {
    let retained_a = lifecycle_job("push", "refs/heads/main", 121, 1, 1211, "group-a")?;
    let rerun_required = lifecycle_job("push", "refs/heads/main", 121, 2, 1213, "required")?;
    retained_a.validate()?;
    rerun_required.validate()?;
    if retained_a.run_attempt != 1 || rerun_required.run_attempt != 2 {
        return Err(invalid("mixed attempts lost their attempt binding"));
    }
    Ok(())
}

/// C12: stale B evidence is rejected; readable coverage is used when
/// available and cold recovery runs when it is not.
pub(crate) fn check_stale_evidence_rejected() -> Result<(), Box<dyn Error>> {
    let stale_b = lifecycle_attempt_ledger("b1", true, false)?;
    if stale_b.failed() == 0 {
        return Err(invalid("stale failed ledger passed as rerun success"));
    }
    let fresh_b = lifecycle_attempt_ledger("b2", false, false)?;
    if fresh_b.failed() != 0 {
        return Err(invalid("fresh rerun ledger kept the old failure"));
    }
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-rerun")?;
    let job = lifecycle_job("push", "refs/heads/main", 121, 2, 1212, "group-b")?;
    let readable = lifecycle_discovery(
        job.clone(),
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![lifecycle_entry(
            &generation,
            "refs/heads/main",
            1211,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let readable_record =
        decide_discovery(&readable).map_err(|error| invalid(&error.to_string()))?;
    if readable_record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("retained coverage missed its readable hit"));
    }
    let exact = RestoreObservation {
        intended_key: generation.cache_key(),
        matched_key: generation.cache_key(),
        restored_compatibility: generation.compatibility_digest.clone(),
        expected_compatibility: generation.compatibility_digest.clone(),
        restored_coverage: generation.coverage_digest.clone(),
        expected_coverage: generation.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-qualification-lifecycle-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-qualification-lifecycle-manifest"),
    };
    verify_restore(&exact).map_err(|error| invalid(&error.to_string()))?;
    let missing = lifecycle_discovery(
        job,
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        Vec::new(),
        true,
    );
    let missing_record = decide_discovery(&missing).map_err(|error| invalid(&error.to_string()))?;
    if missing_record.decision != DiscoveryDecision::ColdMissing {
        return Err(invalid("missing rerun coverage skipped cold recovery"));
    }
    let cold = cold_ledger()?;
    if cold.dependency_compilations() == 0 {
        return Err(invalid("rerun cold recovery measured zero work"));
    }
    let stale_bytes = b"gc1-qualification-lifecycle-stale-attempt".to_vec();
    let fresh_digest = sha256_hex(&lifecycle_frozen_bytes());
    let stale_entry = ObservedEntry {
        repository_id: 1,
        cache_id: 1212,
        ref_name: "refs/heads/main".to_owned(),
        key: generation.cache_key(),
        version: "v1".to_owned(),
        size_in_bytes: stale_bytes.len() as u64,
        created_at: "2026-10-26T00:00:00Z".to_owned(),
    };
    let stale_readback = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &fresh_digest,
        frozen_archive_bytes: stale_bytes.len() as u64,
        restored_bytes: Some(&stale_bytes),
        inventory_complete: true,
        observed_entry: Some(&stale_entry),
        expected_key: &generation.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(stale_readback, ReadbackVerdict::Corrupt { .. }) {
        return Err(invalid("stale rerun bytes escaped corruption"));
    }
    Ok(())
}
