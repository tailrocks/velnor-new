// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use crate::gc1_qualification_support::{
    ApprovalState, FixtureCacheRef, HarnessMode, HostedCommand, ObservationClosure,
    QUALIFICATION_WORKFLOW_FILE, Workload, harness_mode, invalid, require_complete_observation,
    require_dispatchable, require_fault_dispatchable, require_nonempty_workload,
    require_qualification_destructive,
};

fn must_fail<T, E>(result: &Result<T, E>, case: &str) -> Result<(), Box<dyn Error>> {
    if result.is_err() {
        return Ok(());
    }
    Err(invalid(case))
}

fn fixture_workload() -> Workload {
    Workload {
        recipe_ids: vec!["fixture-recipe".to_owned()],
        eligible_dependency_ids: vec!["fixture-dependency".to_owned()],
    }
}

fn fixture_observation() -> ObservationClosure {
    ObservationClosure {
        complete: true,
        detail: "ledger-drained local fixture observation".to_owned(),
    }
}

fn fixture_inputs() -> BTreeMap<String, String> {
    let mut inputs = BTreeMap::new();
    inputs.insert("mode".to_owned(), "both".to_owned());
    inputs
}

fn unapproved_command() -> HostedCommand {
    HostedCommand {
        workflow_file: QUALIFICATION_WORKFLOW_FILE.to_owned(),
        case_ids: vec!["C01".to_owned()],
        inputs: fixture_inputs(),
        argv: vec![
            "gh".to_owned(),
            "workflow".to_owned(),
            "run".to_owned(),
            "qualification.yml".to_owned(),
        ],
        max_runs: 1,
        fault_allowance: 1,
        approval: ApprovalState::Unapproved {
            reason: "no campaign approval recorded".to_owned(),
        },
    }
}

fn approved_command() -> HostedCommand {
    HostedCommand {
        workflow_file: QUALIFICATION_WORKFLOW_FILE.to_owned(),
        case_ids: vec!["C01".to_owned()],
        inputs: fixture_inputs(),
        argv: vec![
            "gh".to_owned(),
            "workflow".to_owned(),
            "run".to_owned(),
            "qualification.yml".to_owned(),
        ],
        max_runs: 1,
        fault_allowance: 1,
        approval: ApprovalState::Approved {
            approver: "fixture-approver".to_owned(),
            campaign_id: "fixture-campaign".to_owned(),
        },
    }
}

pub(crate) mod gc1_aux {
    use super::*;

    #[test]
    fn rejects_empty_workload() -> Result<(), Box<dyn Error>> {
        require_nonempty_workload(&fixture_workload())?;
        require_complete_observation(&fixture_observation())?;
        must_fail(
            &require_nonempty_workload(&Workload {
                recipe_ids: Vec::new(),
                eligible_dependency_ids: vec!["fixture-dependency".to_owned()],
            }),
            "empty recipe set admitted",
        )?;
        must_fail(
            &require_nonempty_workload(&Workload {
                recipe_ids: vec!["fixture-recipe".to_owned()],
                eligible_dependency_ids: Vec::new(),
            }),
            "empty dependency set admitted",
        )?;
        must_fail(
            &require_complete_observation(&ObservationClosure {
                complete: false,
                detail: "partial fixture observation".to_owned(),
            }),
            "incomplete observation admitted",
        )?;
        must_fail(
            &require_complete_observation(&ObservationClosure {
                complete: true,
                detail: String::new(),
            }),
            "observation without detail admitted",
        )?;
        Ok(())
    }

    #[test]
    fn rejects_unapproved_fault() -> Result<(), Box<dyn Error>> {
        let approved = approved_command();
        require_fault_dispatchable(&approved)?;
        require_dispatchable(&approved)?;
        match approved.inputs.get("mode") {
            Some(mode) if mode == "both" => {}
            _ => return Err(invalid("hosted command lost its workflow inputs")),
        }
        if let ApprovalState::Unapproved { reason } = &unapproved_command().approval {
            if reason.is_empty() {
                return Err(invalid("unapproved command has no reason"));
            }
        } else {
            return Err(invalid("unapproved fixture command reads approved"));
        }
        require_qualification_destructive(
            &FixtureCacheRef {
                cache_id: 7,
                namespace: "velnor-gc1-qualification".to_owned(),
            },
            &[7],
        )?;
        must_fail(
            &require_dispatchable(&unapproved_command()),
            "unapproved hosted command admitted",
        )?;
        must_fail(
            &require_fault_dispatchable(&unapproved_command()),
            "unapproved fault run admitted",
        )?;
        let mut zero_fault = approved_command();
        zero_fault.fault_allowance = 0;
        must_fail(
            &require_fault_dispatchable(&zero_fault),
            "fault run without allowance admitted",
        )?;
        must_fail(
            &require_qualification_destructive(
                &FixtureCacheRef {
                    cache_id: 7,
                    namespace: "production".to_owned(),
                },
                &[7],
            ),
            "production cache ID admitted for destructive fixture use",
        )?;
        must_fail(
            &require_qualification_destructive(
                &FixtureCacheRef {
                    cache_id: 9,
                    namespace: "velnor-gc1-qualification".to_owned(),
                },
                &[7],
            ),
            "unlisted cache ID admitted for destructive fixture use",
        )?;
        match harness_mode() {
            HarnessMode::LocalFixture => Ok(()),
            HarnessMode::ApprovedHosted => Err(invalid(
                "unit test defaulted to hosted dispatch; local fixtures must stay the default",
            )),
        }
    }
}

#[path = "gc1_qualification_inputs.rs"]
mod gc1_qualification_inputs;

#[path = "gc1_qualification_lifecycle.rs"]
mod gc1_qualification_lifecycle;

#[expect(
    clippy::duplicate_mod,
    reason = "T59 exercises input qualification through the shared lifecycle by path include"
)]
#[path = "../src/compiler_observation.rs"]
mod compiler_observation;

#[expect(
    clippy::duplicate_mod,
    reason = "T59 exercises input qualification through the shared lifecycle by path include"
)]
#[expect(
    unreachable_pub,
    reason = "T59 exercises the frozen replay record by path include; the owning task keeps the public surface"
)]
#[path = "../src/cache_coverage_replay.rs"]
mod cache_coverage_replay;

#[expect(
    clippy::duplicate_mod,
    reason = "T59 exercises input qualification through the shared lifecycle by path include"
)]
#[expect(
    unreachable_pub,
    reason = "T59 exercises discovery through the owning task path include; the owning task keeps the public surface"
)]
#[path = "../src/cache_discovery.rs"]
mod cache_discovery;

#[expect(
    clippy::duplicate_mod,
    reason = "T61 exercises coverage growth through the shared intent record by path include"
)]
#[expect(
    unreachable_pub,
    reason = "T61 exercises the active intent record by path include; the owning task keeps the public surface"
)]
#[path = "../src/cache_intent.rs"]
mod cache_intent;

#[path = "gc1_qualification_failure.rs"]
mod gc1_qualification_failure;

#[path = "gc1_qualification_paths.rs"]
mod gc1_qualification_paths;

pub(crate) mod gc1_qualification {
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
        changed_closure_ledger, cold_ledger, compat_variant_generation, coverage_variant,
        eligible_ids, fixture_graph, fixture_workspaces, generation_for, invalid,
        native_cold_ledger, query_only_ledger, require_changed_coverage, require_measured_zero,
        require_nonzero, require_same_coverage, require_valid_digests, require_zero,
        warm_native_ledger,
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
        check_doc_phase_routing, check_family_invocation_validation,
        check_family_ledger_completeness, check_named_lane_identity,
        check_native_command_attribution, check_old_key_never_exact_hits,
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

    #[test]
    fn c01() -> Result<(), Box<dyn Error>> {
        require_nonempty_workload(&workload())?;
        require_complete_observation(&observation("ledger-drained cold observation"))?;
        let report = finish_cold_session()?;
        check_cold_report(&report)?;
        check_rejects_query_control()?;
        Ok(())
    }

    fn frozen_candidate() -> Result<FrozenArchive, Box<dyn Error>> {
        let mut bytes = b"gc1-qualification-frozen-candidate:".to_vec();
        for round in 0_u32..64 {
            bytes.extend_from_slice(&round.to_le_bytes());
        }
        freeze_archive(&bytes).map_err(|error| invalid(&error.to_string()))
    }

    fn replay_proof(frozen: &FrozenArchive) -> Result<FrozenProofV1, Box<dyn Error>> {
        let proof = FrozenProofV1 {
            schema: 1,
            producer_job: fixture_job("writer")?,
            generation: generation_for(b"gc1-qualification-coverage")?,
            manifest_digest: digest_b3_typed(b"gc1-qualification-manifest"),
            frozen_archive_digest: frozen.archive_sha256.clone(),
            frozen_archive_bytes: frozen.archive_bytes,
            frozen_at: "2026-10-26T00:00:00Z".to_owned(),
            paired_input_archive_bindings: vec![
                PairedInputBinding {
                    input_role: "sources".to_owned(),
                    archive_digest: digest_b3_typed(b"gc1-qualification-sources"),
                },
                PairedInputBinding {
                    input_role: "tools".to_owned(),
                    archive_digest: digest_b3_typed(b"gc1-qualification-tools"),
                },
            ],
            restore_context_digest: digest_b3_typed(b"gc1-qualification-restore-context"),
            restored_state_index_digest: digest_b3_typed(b"gc1-qualification-state-index"),
            restored_projection_index_digest: digest_b3_typed(
                b"gc1-qualification-projection-index",
            ),
            metadata_roundtrip_passed: true,
            ephemeral_mbx_initial_state: MbxInitialState::Empty,
            ephemeral_mbx_evidence: "ephemeral MBX store observed empty before replay".to_owned(),
            replay_recipe_ids: vec!["check".to_owned(), "clippy".to_owned(), "test".to_owned()],
            replay_command_ledger_digest: digest_b3_typed(b"gc1-qualification-ledger"),
            replay_observation_digest: digest_b3_typed(b"gc1-qualification-observation"),
            eligible_dependency_ids_digest: digest_b3_typed(b"gc1-qualification-eligible"),
            eligible_dependency_count: 3,
            unchanged_dependency_compilations: Some(0),
            covered_origin_acquisitions: Some(0),
            observation_gaps: Vec::new(),
            required_outputs_present: true,
            archive_digest_after_replay: frozen.archive_sha256.clone(),
            completed_coverage_digest: digest_b3_typed(b"gc1-qualification-completed"),
            qualification_outcome: QualificationOutcome::Pass,
        };
        verify_replay_frozen(&proof, frozen).map_err(|error| invalid(&error.to_string()))?;
        Ok(proof)
    }

    fn check_frozen_replay() -> Result<(), Box<dyn Error>> {
        let frozen = frozen_candidate()?;
        let proof = replay_proof(&frozen)?;
        if proof.eligible_dependency_count == 0 {
            return Err(invalid("replay lost its nonempty eligible set"));
        }
        let bytes = replay_canonical_bytes(&proof).map_err(|error| invalid(&error.to_string()))?;
        let parsed = parse_replay_bytes(&bytes).map_err(|error| invalid(&error.to_string()))?;
        if parsed != proof {
            return Err(invalid("frozen proof lost its strict round trip"));
        }
        if replay_digest(&proof).map_err(|error| invalid(&error.to_string()))?
            != replay_digest(&parsed).map_err(|error| invalid(&error.to_string()))?
        {
            return Err(invalid("frozen proof digest drifted across round trip"));
        }
        Ok(())
    }

    fn selectable_entry(generation: &GenerationId) -> SelectableEntry {
        SelectableEntry {
            cache_id: 7,
            repository_id: 1,
            ref_name: "refs/heads/main".to_owned(),
            key: generation.cache_key(),
            version: "v1".to_owned(),
            size_in_bytes: 512,
            created_at: "2026-10-25T00:00:00Z".to_owned(),
            provenance: EntryProvenance::Trusted,
        }
    }

    fn discovery_request_for(generation: GenerationId) -> Result<DiscoveryRequest, Box<dyn Error>> {
        Ok(DiscoveryRequest {
            job: fixture_job("reader")?,
            generation,
            expected_cache_version: "v1".to_owned(),
            allowed_refs: vec!["refs/heads/main".to_owned()],
            current_ref: "refs/heads/main".to_owned(),
            inventory_digest: digest_b3_typed(b"gc1-qualification-inventory"),
            inventory_complete: true,
            entries: vec![selectable_entry(&generation_for(
                b"gc1-qualification-coverage",
            )?)],
            active_coverage_ids: vec!["coverage-a".to_owned()],
        })
    }

    fn check_exact_discovery_first() -> Result<(), Box<dyn Error>> {
        let generation = generation_for(b"gc1-qualification-coverage")?;
        let request = discovery_request_for(generation.clone())?;
        let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
        if record.decision != DiscoveryDecision::ExactHit {
            return Err(invalid("exact requested key lost first position"));
        }
        if record.shadow_state != ShadowState::Clear {
            return Err(invalid("clean inventory reported a shadow"));
        }
        let candidate = record
            .exact_candidate
            .as_ref()
            .ok_or_else(|| invalid("exact hit lost its candidate"))?;
        if candidate.entry.key != generation.cache_key() {
            return Err(invalid("exact hit kept the wrong candidate"));
        }
        let bytes =
            discovery_canonical_bytes(&record).map_err(|error| invalid(&error.to_string()))?;
        let parsed = parse_discovery_bytes(&bytes).map_err(|error| invalid(&error.to_string()))?;
        if parsed != record {
            return Err(invalid("discovery lost its strict round trip"));
        }
        if discovery_digest(&record).map_err(|error| invalid(&error.to_string()))?
            != discovery_digest(&parsed).map_err(|error| invalid(&error.to_string()))?
        {
            return Err(invalid("discovery digest drifted across round trip"));
        }
        let observation = RestoreObservation {
            intended_key: generation.cache_key(),
            matched_key: candidate.entry.key.clone(),
            restored_compatibility: generation.compatibility_digest.clone(),
            expected_compatibility: generation.compatibility_digest.clone(),
            restored_coverage: generation.coverage_digest.clone(),
            expected_coverage: generation.coverage_digest.clone(),
            restored_manifest: digest_b3_typed(b"gc1-qualification-manifest"),
            intended_manifest: digest_b3_typed(b"gc1-qualification-manifest"),
        };
        verify_restore(&observation).map_err(|error| invalid(&error.to_string()))?;
        Ok(())
    }

    fn declared_entry(path: &str, bytes: &[u8], mode: u32) -> ProjectionEntry {
        ProjectionEntry {
            path: path.to_owned(),
            kind: EntryKind::File,
            mode,
            content_sha256: sha256_hex(bytes),
            link_target: None,
            mtime: Some(RealMtime {
                seconds: 946_684_800,
                nanoseconds: 123_456_789,
            }),
        }
    }

    fn write_checkout_file(
        root: &Path,
        path: &str,
        bytes: &[u8],
        mode: u32,
    ) -> Result<PathBuf, Box<dyn Error>> {
        let full = root.join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&full, bytes)?;
        fs::set_permissions(&full, fs::Permissions::from_mode(mode))?;
        Ok(full)
    }

    fn check_fresh_runner_restore() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let checkout = temp.path().join("checkout");
        fs::create_dir_all(&checkout)?;
        let declared = declared_entry("src/lib.rs", b"restored-input", 0o644);
        declared.validate()?;
        if authenticate_entry(&checkout, &declared)? != CheckoutState::Missing {
            return Err(invalid("empty fresh runner misclassified its state"));
        }
        write_checkout_file(&checkout, "src/lib.rs", b"restored-input", 0o644)?;
        if authenticate_entry(&checkout, &declared)? != CheckoutState::Unchanged {
            return Err(invalid("restored input rejected on a fresh runner"));
        }
        if decide(Some(&declared), Some(&declared), CheckoutState::Unchanged)
            != RestoreDecision::Restore
        {
            return Err(invalid("unchanged input not scheduled for restore"));
        }
        Ok(())
    }

    fn check_rejects_false_warm() -> Result<(), Box<dyn Error>> {
        let frozen = frozen_candidate()?;
        let proof = replay_proof(&frozen)?;
        let mut wrong_digest = proof.clone();
        wrong_digest.frozen_archive_digest = "0".repeat(64);
        must_fail(
            &verify_replay_frozen(&wrong_digest, &frozen).map_err(|_| invalid("foreign")),
            "foreign frozen digest admitted",
        )?;
        let mut wrong_bytes = proof.clone();
        wrong_bytes.frozen_archive_bytes += 1;
        must_fail(
            &verify_replay_frozen(&wrong_bytes, &frozen).map_err(|_| invalid("drift")),
            "frozen byte count drift admitted",
        )?;
        let mut mutated = proof.clone();
        mutated.archive_digest_after_replay = "1".repeat(64);
        must_fail(
            &verify_replay_frozen(&mutated, &frozen).map_err(|_| invalid("mutated")),
            "replay-mutated archive admitted",
        )?;
        let mut compiled = proof.clone();
        compiled.unchanged_dependency_compilations = Some(2);
        must_fail(
            &verify_replay_frozen(&compiled, &frozen).map_err(|_| invalid("compiled")),
            "unchanged dependency compilation admitted as warm",
        )?;
        let mut unknown = proof.clone();
        unknown.unchanged_dependency_compilations = None;
        must_fail(
            &verify_replay_frozen(&unknown, &frozen).map_err(|_| invalid("unknown")),
            "unknown compilations admitted as warm zero",
        )?;
        let mut gaps = proof.clone();
        gaps.observation_gaps = vec!["native coverage incomplete".to_owned()];
        must_fail(
            &verify_replay_frozen(&gaps, &frozen).map_err(|_| invalid("gaps")),
            "gapped observation admitted as warm zero",
        )?;
        let generation = generation_for(b"gc1-qualification-coverage")?;
        let mut mismatch = RestoreObservation {
            intended_key: generation.cache_key(),
            matched_key: generation.cache_key(),
            restored_compatibility: generation.compatibility_digest.clone(),
            expected_compatibility: generation.compatibility_digest.clone(),
            restored_coverage: generation.coverage_digest.clone(),
            expected_coverage: generation.coverage_digest.clone(),
            restored_manifest: digest_b3_typed(b"gc1-qualification-manifest"),
            intended_manifest: digest_b3_typed(b"gc1-qualification-manifest"),
        };
        mismatch.matched_key = compat_variant_generation()?.cache_key();
        must_fail(
            &verify_restore(&mismatch).map_err(|_| invalid("key")),
            "mismatched restore key admitted",
        )?;
        let other = discovery_request_for(compat_variant_generation()?)?;
        let other_record = decide_discovery(&other).map_err(|error| invalid(&error.to_string()))?;
        if other_record.decision == DiscoveryDecision::ExactHit {
            return Err(invalid("incompatible generation kept the exact hit"));
        }
        Ok(())
    }

    #[test]
    fn c02() -> Result<(), Box<dyn Error>> {
        require_nonempty_workload(&workload())?;
        require_complete_observation(&observation("frozen fresh-runner replay"))?;
        check_frozen_replay()?;
        check_exact_discovery_first()?;
        check_fresh_runner_restore()?;
        check_rejects_false_warm()?;
        Ok(())
    }

    fn check_app_only_preserves_coverage() -> Result<(), Box<dyn Error>> {
        let base = base_coverage()?;
        let app_only = coverage_variant("0.1.1", "0.2.0", None, LINUX_TARGET, None, 'f')?;
        require_valid_digests(&base)?;
        require_valid_digests(&app_only)?;
        require_same_coverage(&base, &app_only)?;
        let dep_bump = coverage_variant("0.1.0", "0.3.0", None, LINUX_TARGET, None, 'f')?;
        require_changed_coverage(&base, &dep_bump)?;
        if base.workspace_digest == dep_bump.workspace_digest {
            return Err(invalid("dependency change kept the workspace digest"));
        }
        Ok(())
    }

    fn check_projection_restore() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let checkout = temp.path().join("checkout");
        fs::create_dir_all(&checkout)?;
        let declared = declared_entry("src/lib.rs", b"fixture-bytes", 0o644);
        declared.validate()?;
        let mut retimed = declared.clone();
        retimed.mtime = Some(RealMtime {
            seconds: 1_700_000_000,
            nanoseconds: 1,
        });
        if !declared.semantic_matches(&retimed) {
            return Err(invalid("restored mtimes leaked into coverage identity"));
        }
        write_checkout_file(&checkout, "src/lib.rs", b"other bytes", 0o644)?;
        if authenticate_entry(&checkout, &declared)? != CheckoutState::Changed {
            return Err(invalid("content mismatch missed"));
        }
        write_checkout_file(&checkout, "src/lib.rs", b"fixture-bytes", 0o600)?;
        if authenticate_entry(&checkout, &declared)? != CheckoutState::Changed {
            return Err(invalid("mode mismatch missed"));
        }
        write_checkout_file(&checkout, "src/lib.rs", b"fixture-bytes", 0o644)?;
        if authenticate_entry(&checkout, &declared)? != CheckoutState::Unchanged {
            return Err(invalid("identical restored file rejected"));
        }
        if decide(Some(&declared), Some(&declared), CheckoutState::Changed)
            != RestoreDecision::KeepCurrent
        {
            return Err(invalid("changed checkout file not kept current"));
        }
        if decide(Some(&declared), Some(&declared), CheckoutState::Missing)
            != RestoreDecision::KeepCurrent
        {
            return Err(invalid("deleted input resurrected from cache"));
        }
        if decide(Some(&declared), None, CheckoutState::Unchanged) != RestoreDecision::Discard {
            return Err(invalid("cached-only path kept"));
        }
        let git_entry = ProjectionEntry {
            path: ".git/objects/pack".to_owned(),
            ..declared_entry("src/lib.rs", b"pack", 0o644)
        };
        if !is_git_path(&git_entry.path) {
            return Err(invalid("git path not detected"));
        }
        if decide(Some(&git_entry), Some(&git_entry), CheckoutState::Unchanged)
            != RestoreDecision::KeepCurrent
        {
            return Err(invalid("checkout git metadata scheduled for restore"));
        }
        Ok(())
    }

    fn check_rejects_tampered_projection() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let checkout = temp.path().join("checkout");
        let staged = temp.path().join("staged");
        fs::create_dir_all(&checkout)?;
        fs::create_dir_all(&staged)?;
        let declared = declared_entry("src/lib.rs", b"declared", 0o644);
        write_checkout_file(&checkout, "src/lib.rs", b"declared", 0o644)?;
        let mut tampered = declared.clone();
        tampered.content_sha256 = sha256_hex(b"tampered");
        must_fail(
            &reconcile(&tampered, &declared).map(|_| ()),
            "archive mismatch admitted",
        )?;
        let mut timeless = declared.clone();
        timeless.mtime = None;
        must_fail(
            &reconcile(&timeless, &declared).map(|_| ()),
            "missing mtime admitted",
        )?;
        let staged_file = write_checkout_file(&staged, "src/lib.rs", b"tampered", 0o644)?;
        let destination = staged_path(&checkout, "src/lib.rs")?;
        must_fail(
            &install_staged_file(&staged_file, &destination, &declared).map(|_| ()),
            "tampered staged file installed",
        )?;
        if fs::read(&destination)? != b"declared" {
            return Err(invalid("failed install wrote destination"));
        }
        Ok(())
    }

    #[test]
    fn c03() -> Result<(), Box<dyn Error>> {
        require_nonempty_workload(&workload())?;
        require_complete_observation(&observation("application-only change"))?;
        check_app_only_preserves_coverage()?;
        check_projection_restore()?;
        check_rejects_tampered_projection()?;
        Ok(())
    }

    fn check_closure_partition() -> Result<(), Box<dyn Error>> {
        let closure = app_closure()?;
        for member in [APP_ID, LIB_ID, SERDE_ID] {
            if !closure.contains(member) {
                return Err(invalid("affected closure lost a member"));
            }
        }
        if closure.len() != 3 {
            return Err(invalid("affected closure grew beyond the fixture"));
        }
        require_nonzero("affected closure size", closure.len() as u64)?;
        let workspaces = fixture_workspaces("0.1.0", "0.2.0");
        if workspaces.len() != 1 {
            return Err(invalid("workspace fixture grew beyond the case"));
        }
        for member in [APP_ID, LIB_ID] {
            if !workspaces[0].members.contains(&member.to_owned()) {
                return Err(invalid("workspace fixture lost a member"));
            }
        }
        Ok(())
    }

    fn check_single_dep_update() -> Result<(), Box<dyn Error>> {
        let base: DependencyCoverage = base_coverage()?;
        let lock_bump = coverage_variant("0.1.0", "0.2.0", None, LINUX_TARGET, None, '1')?;
        if base.dependency_digest == lock_bump.dependency_digest {
            return Err(invalid("dependency update kept the old dependency digest"));
        }
        if base.coverage_digest == lock_bump.coverage_digest {
            return Err(invalid("dependency update kept the old coverage digest"));
        }
        let app_only = coverage_variant("0.1.1", "0.2.0", None, LINUX_TARGET, None, 'f')?;
        require_same_coverage(&base, &app_only)?;
        let changed = changed_closure_ledger()?;
        require_nonzero(
            "changed-closure compilations",
            changed.dependency_compilations(),
        )?;
        require_zero(
            "changed-closure application compilations",
            changed.application_compilations(),
        )?;
        let closure = resolve_dependency_closure("c04-task", &base)?;
        closure.verify_complete()?;
        let mut keys: Vec<&str> = closure.inputs.keys().map(String::as_str).collect();
        keys.sort_unstable();
        if keys
            != vec![
                "coverage",
                "dependencies",
                "native_inputs",
                "workspace_deps",
            ]
        {
            return Err(invalid("dependency closure lost an input class"));
        }
        Ok(())
    }

    fn check_rejects_unknown_closure() -> Result<(), Box<dyn Error>> {
        must_fail(
            &dependency_closure_ids(&fixture_graph(), "").map(|_| ()),
            "empty package admitted",
        )?;
        must_fail(
            &dependency_closure_ids(&fixture_graph(), "missing-crate").map(|_| ()),
            "unknown package admitted",
        )?;
        let base = base_coverage()?;
        must_fail(
            &resolve_dependency_closure("", &base).map(|_| ()),
            "empty task admitted",
        )?;
        let mut malformed = base.clone();
        "not-a-digest".clone_into(&mut malformed.coverage_digest);
        must_fail(
            &resolve_dependency_closure("c04-task", &malformed).map(|_| ()),
            "malformed coverage admitted",
        )?;
        Ok(())
    }

    #[test]
    fn c04() -> Result<(), Box<dyn Error>> {
        require_nonempty_workload(&workload())?;
        require_complete_observation(&observation("single dependency update"))?;
        check_closure_partition()?;
        check_single_dep_update()?;
        check_rejects_unknown_closure()?;
        Ok(())
    }

    fn check_feature_profile_changes() -> Result<(), Box<dyn Error>> {
        let base = base_coverage()?;
        let features = coverage_variant("0.1.0", "0.2.0", Some("tokio"), LINUX_TARGET, None, 'f')?;
        if base.dependency_digest == features.dependency_digest {
            return Err(invalid("feature change kept the old dependency digest"));
        }
        if base.coverage_digest == features.coverage_digest {
            return Err(invalid("feature change kept the old coverage digest"));
        }
        if base.workspace_digest != features.workspace_digest {
            return Err(invalid("feature change moved the workspace digest"));
        }
        if base.native_digest != features.native_digest {
            return Err(invalid("feature change moved the native digest"));
        }
        let targets = coverage_variant("0.1.0", "0.2.0", None, ARM_TARGET, None, 'f')?;
        if base.dependency_digest == targets.dependency_digest {
            return Err(invalid("target change kept the old dependency digest"));
        }
        if base.coverage_digest == targets.coverage_digest {
            return Err(invalid("target change kept the old coverage digest"));
        }
        let native = coverage_variant("0.1.0", "0.2.0", None, LINUX_TARGET, Some("libgit2"), 'f')?;
        if base.native_digest == native.native_digest {
            return Err(invalid("native change kept the old native digest"));
        }
        if base.coverage_digest == native.coverage_digest {
            return Err(invalid("native change kept the old coverage digest"));
        }
        if base.dependency_digest != native.dependency_digest {
            return Err(invalid("native change moved the dependency digest"));
        }
        Ok(())
    }

    fn check_runner_profile_separation() -> Result<(), Box<dyn Error>> {
        if NextestProfile::Ci.as_str() == NextestProfile::Default.as_str() {
            return Err(invalid("nextest profiles lost their distinction"));
        }
        if NextestProfile::parse("ci")? != NextestProfile::Ci {
            return Err(invalid("nextest profile lost its round trip"));
        }
        if NextestProfile::parse("default")? != NextestProfile::Default {
            return Err(invalid("default profile lost its round trip"));
        }
        must_fail(
            &NextestProfile::parse("nightly").map(|_| ()),
            "unknown nextest profile admitted",
        )?;
        if CompileDriver::Cargo.as_str() == NextestProfile::Default.as_str() {
            return Err(invalid("compile driver merged with the test profile"));
        }
        if CompileDriver::parse("cargo")? != CompileDriver::Cargo {
            return Err(invalid("compile driver lost its round trip"));
        }
        must_fail(
            &CompileDriver::parse("turbo").map(|_| ()),
            "unknown compile driver admitted",
        )?;
        if TestRunner::CargoNextest == TestRunner::CargoTest {
            return Err(invalid("test runners lost their distinction"));
        }
        if TestRunner::parse("cargo_nextest")? != TestRunner::CargoNextest {
            return Err(invalid("test runner lost its round trip"));
        }
        must_fail(
            &TestRunner::parse("jest").map(|_| ()),
            "unknown test runner admitted",
        )?;
        Ok(())
    }

    #[test]
    fn c05() -> Result<(), Box<dyn Error>> {
        require_nonempty_workload(&workload())?;
        require_complete_observation(&observation("feature and profile change"))?;
        check_feature_profile_changes()?;
        check_runner_profile_separation()?;
        Ok(())
    }

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
}
