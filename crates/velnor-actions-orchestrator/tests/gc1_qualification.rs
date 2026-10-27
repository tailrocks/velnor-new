// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use velnor_actions_contract::canonical::{Digest, digest_b3, digest_b3_typed};
use velnor_actions_contract::workflow::cache_context::JobBindingV1;
use velnor_actions_contract::workflow::cache_identity::GenerationId;
use velnor_actions_orchestrator::{
    CheckoutState, EntryKind, ProjectionEntry, RealMtime, RestoreDecision, authenticate_entry,
    decide, install_staged_file, is_git_path, reconcile, sha256_hex, staged_path,
};
use velnor_actions_rust::closure::resolve_dependency_closure;
use velnor_actions_rust::identity::{DependencyCoverage, dependency_closure_ids};
use velnor_actions_rust::profile::{CompileDriver, NextestProfile, TestRunner};

use super::cache_coverage_replay::{
    FrozenArchive, FrozenProofV1, MbxInitialState, PairedInputBinding, QualificationOutcome,
    freeze_archive, parse_replay_bytes, replay_canonical_bytes, replay_digest,
    verify_replay_frozen,
};
use super::cache_discovery::{
    DiscoveryDecision, DiscoveryRequest, EntryProvenance, RestoreObservation, SelectableEntry,
    ShadowState, decide_discovery, discovery_canonical_bytes, discovery_digest,
    parse_discovery_bytes, verify_restore,
};
use super::compiler_observation::{
    ChildOutcome, ObservationReport, ObservationSession, new_session_id,
};
use super::gc1_qualification_failure::{
    check_absent_states_stay_unavailable, check_admission_blocks_uncertain,
    check_admission_exact_fit_and_overflow, check_admission_slots_and_repair,
    check_completed_reader_keeps_skipped_writer, check_corrupt_inner_rejected,
    check_coverage_growth_keeps_prior, check_current_intent_gates_mutation,
    check_denied_absent_never_claims_entry, check_denied_save_keeps_predecessor,
    check_denied_save_rejects_publication, check_failure_harness,
    check_growth_compiles_only_missing, check_hard_failed_writer_blocks_retroactive,
    check_lookup_and_restore_recency_observed, check_new_generation_publishes_after_replay,
    check_old_key_immutable_until_replaced, check_optional_cache_error_recovers_cold,
    check_restore_timeout_needs_retry, check_skipped_writer_keeps_required,
    check_two_slot_repair_deletes_exact_b, check_verified_bytes_stay_inside_envelope,
};
use super::gc1_qualification_inputs::{
    APP_ID, ARM_TARGET, LIB_ID, LINUX_TARGET, SERDE_ID, app_closure, base_coverage,
    changed_closure_ledger, cold_ledger, compat_variant_generation, coverage_variant, eligible_ids,
    fixture_graph, fixture_workspaces, generation_for, invalid, native_cold_ledger,
    query_only_ledger, require_changed_coverage, require_measured_zero, require_nonzero,
    require_same_coverage, require_valid_digests, require_zero, warm_native_ledger,
};
use super::gc1_qualification_lifecycle::{
    check_app_rerun_reuses_trusted_baseline, check_cancelled_observations_stay_interrupted,
    check_cleanup_cancel_keeps_replacement, check_competing_requests_share_missing,
    check_dependency_change_rebuilds_until_trusted, check_event_trust_transitions,
    check_first_verified_writer_wins, check_fork_cache_read_only, check_fork_event_restricted,
    check_inventory_unavailable_uses_cold_path, check_lifecycle_harness,
    check_longer_prefix_shadow, check_merge_consumers_read_only, check_mixed_attempt_ledgers,
    check_no_global_attempt_equality, check_pin_extracts_matched_key,
    check_pr_output_never_promotes_prefix, check_pr_runs_never_publish,
    check_preparation_cancel_never_completes, check_preview_stable_consume_prepared,
    check_private_source_denied_cold_recovery, check_ref_shadow_refused,
    check_retained_success_accepted, check_stale_evidence_rejected,
    check_upload_cancel_marks_interrupted, check_writer_queue_policy,
};
use super::gc1_qualification_paths::{
    check_all_families_recorded, check_authority_needs_unchanged_intent,
    check_authority_recovers_cold_without_state, check_changed_helper_rebuilds,
    check_doc_phase_routing, check_family_invocation_validation, check_family_ledger_completeness,
    check_named_lane_identity, check_native_command_attribution, check_old_key_never_exact_hits,
    check_overlap_charges_old_and_new, check_paths_harness, check_private_home_restore,
    check_repository_ceiling_counts_all_families, check_tool_kind_coverage,
    check_unchanged_inputs_reuse_eligible, check_unknown_pressure_blocks,
    check_verified_replacement_gates_owned_delete,
};
use super::must_fail;
use crate::gc1_qualification_support::{
    ObservationClosure, Workload, require_complete_observation, require_nonempty_workload,
};

fn workload() -> Workload {
    Workload {
        recipe_ids: vec!["check".to_owned(), "clippy".to_owned(), "test".to_owned()],
        eligible_dependency_ids: eligible_ids(),
    }
}

fn observation(detail: &str) -> ObservationClosure {
    ObservationClosure {
        complete: true,
        detail: detail.to_owned(),
    }
}

fn session_error(error: impl std::fmt::Display) -> Box<dyn Error> {
    invalid(&error.to_string())
}

fn fixture_job(key: &str) -> Result<JobBindingV1, Box<dyn Error>> {
    Ok(JobBindingV1 {
        schema: 1,
        repository_id: 1,
        repository: "tailrocks/velnor-new".to_owned(),
        run_id: 7,
        run_attempt: 1,
        job_id: 11,
        job_key: key.to_owned(),
        event: "push".to_owned(),
        ref_name: "refs/heads/main".to_owned(),
        workflow_sha: format!("{:040x}", 0xa5a5),
        source_sha: format!("{:040x}", 0x5a5a),
        plan_digest: Digest::parse(&digest_b3(b"gc1-qualification-plan"))?,
    })
}

fn finish_cold_session() -> Result<ObservationReport, Box<dyn Error>> {
    let ledger = cold_ledger()?;
    require_nonzero(
        "cold dependency compilations",
        ledger.dependency_compilations(),
    )?;
    require_nonzero(
        "cold application compilations",
        ledger.application_compilations(),
    )?;
    require_zero("cold compiler queries", ledger.queries())?;
    let native = native_cold_ledger()?;
    require_nonzero("cold native compilations", native.dependency_compilations())?;
    let mut session = ObservationSession::begin(
        "gc1-qualification-c01",
        "cold-control",
        &new_session_id(),
        &digest_b3(b"gc1-qualification-c01-context"),
    )
    .map_err(session_error)?;
    let compiler = session
        .compiler_evidence(
            Some(ledger.dependency_compilations()),
            Some(ledger.application_compilations()),
            Some(ledger.queries()),
            Some(ledger.failed()),
            Some(ledger.interrupted()),
            "ledger-drained cold compiler events",
        )
        .map_err(session_error)?;
    session.provide_compiler(compiler).map_err(session_error)?;
    let observed = session
        .native_evidence(
            native.measured_native_compilations(),
            Some(native.queries()),
            Some(native.failed()),
            Some(native.interrupted()),
            "ledger-drained cold native calls",
        )
        .map_err(session_error)?;
    session.provide_native(observed).map_err(session_error)?;
    let origin = session
        .origin_evidence(Some(2), Some(1_048_576), "empty-home acquisition baseline")
        .map_err(session_error)?;
    session.provide_origin(origin).map_err(session_error)?;
    let cache = session
        .cache_evidence(Some(0), "no cache entries restored")
        .map_err(session_error)?;
    session.provide_cache(cache).map_err(session_error)?;
    session.finish(ChildOutcome::Success).map_err(session_error)
}

fn check_cold_report(report: &ObservationReport) -> Result<(), Box<dyn Error>> {
    report.validate().map_err(session_error)?;
    if report.dependency_compilations.value != Some(2) {
        return Err(invalid("cold dependency count drifted"));
    }
    if report.application_compilations.value != Some(1) {
        return Err(invalid("cold application count drifted"));
    }
    if report.native_compilations.value != Some(1) {
        return Err(invalid("cold native count drifted"));
    }
    if report.origin_acquisitions.value != Some(2) {
        return Err(invalid("cold origin acquisitions merged"));
    }
    if report.cache_bytes.value != Some(0) {
        return Err(invalid("cold cache bytes misreported"));
    }
    if !report.gaps.is_empty() {
        return Err(invalid("complete cold observation reported gaps"));
    }
    if report.child_failed {
        return Err(invalid("successful cold run reported as failed"));
    }
    let bytes = report.to_canonical_json().map_err(session_error)?;
    let parsed: ObservationReport =
        serde_json::from_slice(&bytes).map_err(|error| invalid(&error.to_string()))?;
    if parsed != *report {
        return Err(invalid("cold report lost its canonical round trip"));
    }
    Ok(())
}

fn check_rejects_query_control() -> Result<(), Box<dyn Error>> {
    let queries = query_only_ledger()?;
    require_zero(
        "query-only dependency compilations",
        queries.dependency_compilations(),
    )?;
    must_fail(
        &require_nonzero("query-only cold control", queries.dependency_compilations()),
        "compiler version query satisfied the cold control",
    )?;
    must_fail(
        &require_complete_observation(&ObservationClosure {
            complete: false,
            detail: "partial cold observation".to_owned(),
        }),
        "incomplete cold observation admitted",
    )?;
    let mut partial = ObservationSession::begin(
        "gc1-qualification-c01",
        "cold-control",
        &new_session_id(),
        &digest_b3(b"gc1-qualification-c01-context"),
    )
    .map_err(session_error)?;
    let compiler = partial
        .compiler_evidence(None, None, None, None, None, "missing ledger")
        .map_err(session_error)?;
    partial.provide_compiler(compiler).map_err(session_error)?;
    must_fail(
        &partial.finish(ChildOutcome::Success).map_err(session_error),
        "report finished with missing dimensions",
    )?;
    Ok(())
}

#[path = "gc1_qualification_c01_c02.rs"]
mod gc1_qualification_c01_c02;
#[path = "gc1_qualification_c03_c05.rs"]
mod gc1_qualification_c03_c05;
#[path = "gc1_qualification_c06_c22.rs"]
mod gc1_qualification_c06_c22;
pub(crate) use gc1_qualification_c01_c02::*;
