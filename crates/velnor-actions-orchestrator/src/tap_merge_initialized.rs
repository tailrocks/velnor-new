// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    candidate::MergeCandidate,
    cutover::{self, CutoverInput},
    github::TapMergeGitHub,
    observation::{MergeAuthority, check_runs},
    protection::{ProtectionAuthority, exact_required_checks, resolve_protection},
};
use crate::{OrchestratorError, internal::internal, tap_prepare::TapPromotionOutcome};
use velnor_actions_contract::homebrew_merge_gate::{
    TapCompletedMainObservation, TapCompletedPromotionObservation, TapHistoricalFeedback,
    TapServerMergeProtection, validate_tap_completed_promotion_gate,
};
use velnor_actions_mise::CheckDeadline;

pub(super) struct VerifiedCutoverInitialization {
    outcome: TapPromotionOutcome,
}

impl VerifiedCutoverInitialization {
    pub(super) fn into_outcome(self) -> TapPromotionOutcome {
        self.outcome
    }
}

pub(super) fn verify(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    api: &TapMergeGitHub,
    protection_reader: &impl super::protection_observer::ClassicProtectionReader,
) -> Result<VerifiedCutoverInitialization, OrchestratorError> {
    let transition = crate::homebrew::structural_cutover::protected_transition()?;
    if authority.repo_id != transition.transition().tap_repository_id {
        return Err(internal("tap_merge_initialization_repository"));
    }
    if !super::history::capture_all(candidate, authority, api)?.is_empty() {
        return Err(internal(
            "tap_merge_initialization_promotion_history_exists",
        ));
    }
    let remaining = api
        .deadline()
        .saturating_duration_since(std::time::Instant::now());
    let deadline =
        CheckDeadline::after(remaining).map_err(|_| internal("tap_merge_deadline_elapsed"))?;
    let input = CutoverInput {
        policy: &candidate.policy,
        spec: candidate.spec.as_ref(),
        plan: &candidate.plan,
        release: &candidate.release,
        formula: &candidate.formula,
        sdk_artifact_digest: &candidate.sdk_artifact_digest,
    };
    let observed = cutover::observe(&input, api, deadline)?;
    if observed.outcome() != candidate.promotion.outcome {
        return Err(internal("tap_merge_initialization_handoff_changed"));
    }
    verify_gate(candidate, authority, api, protection_reader, &observed)?;
    let final_observed = cutover::observe(&input, api, deadline)?;
    if final_observed.outcome() != observed.outcome()
        || final_observed.current_main != observed.current_main
    {
        return Err(internal("tap_merge_initialization_changed"));
    }
    verify_gate(
        candidate,
        authority,
        api,
        protection_reader,
        &final_observed,
    )?;
    if !super::history::capture_all(candidate, authority, api)?.is_empty() {
        return Err(internal(
            "tap_merge_initialization_promotion_history_exists",
        ));
    }
    Ok(VerifiedCutoverInitialization {
        outcome: final_observed.outcome(),
    })
}

fn verify_gate(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    api: &TapMergeGitHub,
    protection_reader: &impl super::protection_observer::ClassicProtectionReader,
    observed: &cutover::ObservedCutoverInitialization,
) -> Result<(), OrchestratorError> {
    let protection = resolve_protection(
        api,
        protection_reader,
        &candidate.policy.repository,
        "main",
        &ProtectionAuthority {
            writer_app_id: authority.writer_app_id,
            writer_bot_user_id: authority.bot_user_id,
            writer_bot_login: &authority.bot_login,
            completion_check_name: &authority.completion_name,
            completion_app_id: authority.completion_app_id,
        },
    )?;
    let runs = check_runs(api, &candidate.policy.repository, &observed.pull.head)?;
    let checks = exact_required_checks(&protection, &runs, &observed.pull.head)?
        .ok_or_else(|| internal("tap_merge_initialization_checks_incomplete"))?;
    let captured = capture_feedback(candidate, authority, api, observed, &runs)?;
    let gate = completion_observation(observed, captured, &protection, checks);
    validate_tap_completed_promotion_gate(&authority.policy, &gate)
        .map_err(|_| internal("tap_merge_initialization_gate_rejected"))
}

fn capture_feedback(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    api: &TapMergeGitHub,
    observed: &cutover::ObservedCutoverInitialization,
    runs: &[serde_json::Value],
) -> Result<super::feedback::CapturedFeedback, OrchestratorError> {
    super::feedback::capture_feedback(
        api,
        &super::feedback::FeedbackContext {
            repository: &candidate.policy.repository,
            pull_request_number: observed.pull.number,
            head_sha: &observed.pull.head,
            completion_app_id: authority.completion_app_id,
            completion_check_name: &authority.completion_name,
            expected_reviewer_app_ids: &authority.reviewer_ids,
        },
        runs,
    )
}

fn completion_observation(
    observed: &cutover::ObservedCutoverInitialization,
    captured: super::feedback::CapturedFeedback,
    protection: &super::protection::ProtectionFacts,
    checks: Vec<velnor_actions_contract::homebrew_merge_gate::TapRequiredCheck>,
) -> TapCompletedPromotionObservation {
    TapCompletedPromotionObservation {
        pull_request: TapHistoricalFeedback {
            pull_request_number: observed.pull.number,
            head_sha: observed.pull.head.clone(),
            feedback: captured.snapshot,
            completion: captured.completion,
        },
        validated_base_sha: observed.pull.base.clone(),
        observed_pr_base_sha: observed.pull.base.clone(),
        validated_head_sha: observed.pull.head.clone(),
        merge_sha: observed.pull.landed.clone(),
        main: TapCompletedMainObservation {
            current_sha: observed.current_main.clone(),
            merge_is_ancestor: true,
            formula_matches_receipt: true,
        },
        expected_merged_pull_request: true,
        server_protection: if protection.strict && !protection.writer_bypass {
            TapServerMergeProtection::PullRequestProtected
        } else {
            TapServerMergeProtection::MissingOrBypassable
        },
        required_checks_complete: true,
        checks,
        historical_feedback_complete: true,
        historical_feedback: Vec::new(),
    }
}
