// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_compat_identity() -> Result<(), Box<dyn Error>> {
    let base = generation_for(b"gc1-qualification-coverage")?;
    let variant = compat_variant_generation()?;
    if base.cache_key() == variant.cache_key() {
        return Err(invalid("compatibility change kept the cache key"));
    }
    GenerationId::validate_key(&base.cache_key())?;
    if GenerationId::parse_key(&base.cache_key())? != base {
        return Err(invalid("generation key lost its strict round trip"));
    }
    if !base.cache_key().ends_with("-r0") {
        return Err(invalid("base generation lost its zero repair revision"));
    }
    let repaired = GenerationId {
        repair_revision: 1,
        ..base.clone()
    };
    GenerationId::validate_key(&repaired.cache_key())?;
    if repaired.cache_key() == base.cache_key() {
        return Err(invalid("repair revision did not change the cache key"));
    }
    if !repaired.cache_key().ends_with("-r1") {
        return Err(invalid("repair revision drifted from its bounded key"));
    }
    must_fail(
        &GenerationId::validate_key("bogus-key"),
        "malformed cache key admitted",
    )?;
    if GenerationId::parse_key(&variant.cache_key())? != variant {
        return Err(invalid("variant key lost its strict round trip"));
    }
    Ok(())
}

fn check_native_attribution() -> Result<(), Box<dyn Error>> {
    let cold = native_cold_ledger()?;
    require_nonzero("cold native compilations", cold.dependency_compilations())?;
    must_fail(
        &require_measured_zero("cold native work", cold.measured_native_compilations()),
        "cold native work admitted as warm zero",
    )?;
    let warm = warm_native_ledger();
    require_measured_zero("warm native work", warm.measured_native_compilations())?;
    let mut blind = native_cold_ledger()?;
    blind.record_unobserved_absolute("/usr/bin/cc")?;
    must_fail(
        &require_measured_zero(
            "unobserved native work",
            blind.measured_native_compilations(),
        ),
        "unobserved native calls measured as zero",
    )?;
    Ok(())
}

fn check_rejects_incompatible_restore() -> Result<(), Box<dyn Error>> {
    let generation = generation_for(b"gc1-qualification-coverage")?;
    let exact = RestoreObservation {
        intended_key: generation.cache_key(),
        matched_key: generation.cache_key(),
        restored_compatibility: generation.compatibility_digest.clone(),
        expected_compatibility: generation.compatibility_digest.clone(),
        restored_coverage: generation.coverage_digest.clone(),
        expected_coverage: generation.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-qualification-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-qualification-manifest"),
    };
    verify_restore(&exact).map_err(|error| invalid(&error.to_string()))?;
    let variant = compat_variant_generation()?;
    let mut compat = exact.clone();
    compat.restored_compatibility = variant.compatibility_digest.clone();
    must_fail(&verify_restore(&compat), "incompatible state admitted")?;
    let mut coverage = exact.clone();
    coverage.restored_coverage = generation_for(b"gc1-qualification-other-coverage")?
        .coverage_digest
        .clone();
    must_fail(&verify_restore(&coverage), "foreign coverage admitted")?;
    Ok(())
}

#[test]
fn c06() -> Result<(), Box<dyn Error>> {
    require_nonempty_workload(&workload())?;
    require_complete_observation(&observation("host and compiler change"))?;
    check_compat_identity()?;
    check_native_attribution()?;
    check_rejects_incompatible_restore()?;
    Ok(())
}

#[test]
fn c07() -> Result<(), Box<dyn Error>> {
    check_lifecycle_harness("C07", "PR, main, preview, and stable event sequence")?;
    check_event_trust_transitions()?;
    check_pr_runs_never_publish()?;
    check_app_rerun_reuses_trusted_baseline()?;
    check_dependency_change_rebuilds_until_trusted()?;
    check_pr_output_never_promotes_prefix()?;
    check_preview_stable_consume_prepared()?;
    Ok(())
}

#[test]
fn c08() -> Result<(), Box<dyn Error>> {
    check_lifecycle_harness("C08", "restricted fork PR lifecycle")?;
    check_fork_event_restricted()?;
    check_fork_cache_read_only()?;
    check_private_source_denied_cold_recovery()?;
    Ok(())
}

#[test]
fn c09() -> Result<(), Box<dyn Error>> {
    check_lifecycle_harness("C09", "merge-queue and tag lifecycle")?;
    check_merge_consumers_read_only()?;
    check_ref_shadow_refused()?;
    check_longer_prefix_shadow()?;
    check_pin_extracts_matched_key()?;
    check_inventory_unavailable_uses_cold_path()?;
    Ok(())
}

#[test]
fn c10() -> Result<(), Box<dyn Error>> {
    check_lifecycle_harness("C10", "competing producers lifecycle")?;
    check_writer_queue_policy()?;
    check_competing_requests_share_missing()?;
    check_first_verified_writer_wins()?;
    Ok(())
}

#[test]
fn c11() -> Result<(), Box<dyn Error>> {
    check_lifecycle_harness("C11", "cancellation lifecycle")?;
    check_preparation_cancel_never_completes()?;
    check_upload_cancel_marks_interrupted()?;
    check_cleanup_cancel_keeps_replacement()?;
    check_cancelled_observations_stay_interrupted()?;
    Ok(())
}

#[test]
fn c12() -> Result<(), Box<dyn Error>> {
    check_lifecycle_harness("C12", "failed-job rerun lifecycle")?;
    check_mixed_attempt_ledgers()?;
    check_retained_success_accepted()?;
    check_no_global_attempt_equality()?;
    check_stale_evidence_rejected()?;
    Ok(())
}

#[test]
fn c13() -> Result<(), Box<dyn Error>> {
    check_failure_harness("C13", "skipped and failed producer outcomes")?;
    check_skipped_writer_keeps_required()?;
    check_completed_reader_keeps_skipped_writer()?;
    check_optional_cache_error_recovers_cold()?;
    check_hard_failed_writer_blocks_retroactive()?;
    Ok(())
}

#[test]
fn c14() -> Result<(), Box<dyn Error>> {
    check_failure_harness("C14", "denied save with successful step")?;
    check_denied_save_rejects_publication()?;
    check_denied_absent_never_claims_entry()?;
    check_denied_save_keeps_predecessor()?;
    Ok(())
}

#[test]
fn c15() -> Result<(), Box<dyn Error>> {
    check_failure_harness("C15", "damaged cache faults and repair")?;
    check_absent_states_stay_unavailable()?;
    check_corrupt_inner_rejected()?;
    check_restore_timeout_needs_retry()?;
    check_two_slot_repair_deletes_exact_b()?;
    check_lookup_and_restore_recency_observed()?;
    Ok(())
}

#[test]
fn c16() -> Result<(), Box<dyn Error>> {
    check_failure_harness("C16", "publication near the repository budget")?;
    check_admission_exact_fit_and_overflow()?;
    check_admission_blocks_uncertain()?;
    check_admission_slots_and_repair()?;
    check_verified_bytes_stay_inside_envelope()?;
    Ok(())
}

#[test]
fn c17() -> Result<(), Box<dyn Error>> {
    check_failure_harness("C17", "coverage growth under unchanged compatibility")?;
    check_coverage_growth_keeps_prior()?;
    check_growth_compiles_only_missing()?;
    check_new_generation_publishes_after_replay()?;
    check_old_key_immutable_until_replaced()?;
    check_current_intent_gates_mutation()?;
    Ok(())
}

#[test]
fn c18() -> Result<(), Box<dyn Error>> {
    check_paths_harness("C18", "named checks and native commands with private homes")?;
    check_named_lane_identity()?;
    check_native_command_attribution()?;
    check_private_home_restore()?;
    Ok(())
}

#[test]
fn c19() -> Result<(), Box<dyn Error>> {
    check_paths_harness(
        "C19",
        "every actual test, Clippy, and documentation command family",
    )?;
    check_tool_kind_coverage()?;
    check_doc_phase_routing()?;
    check_family_invocation_validation()?;
    check_family_ledger_completeness()?;
    Ok(())
}

#[test]
fn c20() -> Result<(), Box<dyn Error>> {
    check_paths_harness(
        "C20",
        "release helpers and source-free signing or publishing",
    )?;
    check_changed_helper_rebuilds()?;
    check_unchanged_inputs_reuse_eligible()?;
    check_authority_needs_unchanged_intent()?;
    check_authority_recovers_cold_without_state()?;
    Ok(())
}

#[test]
fn c21() -> Result<(), Box<dyn Error>> {
    check_paths_harness("C21", "migration with old and new namespaces")?;
    check_overlap_charges_old_and_new()?;
    check_old_key_never_exact_hits()?;
    check_verified_replacement_gates_owned_delete()?;
    Ok(())
}

#[test]
fn c22() -> Result<(), Box<dyn Error>> {
    check_paths_harness("C22", "pressure from other existing cache families")?;
    check_all_families_recorded()?;
    check_repository_ceiling_counts_all_families()?;
    check_unknown_pressure_blocks()?;
    Ok(())
}
