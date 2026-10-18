//! Read-only verification for a release already merged into the tap.

use serde_json::Value;
use velnor_actions_contract::homebrew_merge_gate::{
    TapCompletedMainObservation, TapCompletedPromotionObservation, TapServerMergeProtection,
    validate_tap_completed_promotion_gate,
};

use crate::{OrchestratorError, internal::internal};

use super::{
    candidate::{MergeCandidate, validate_formula_at_base},
    github::TapMergeGitHub,
    history::{AuthenticatedPull, capture_all},
    observation::{MergeAuthority, contents},
    protection::{ProtectionAuthority, exact_required_checks, resolve_protection},
};

/// Authenticate an already merged receipt without performing a write.
/// # Errors
/// Rejects any mismatch in the merged PR, checks, formula, ancestry, or review history.
pub(super) fn verify_already_promoted(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    api: &TapMergeGitHub,
) -> Result<(), OrchestratorError> {
    let selection = candidate.selection();
    let Some((expected_merge_sha, expected_merged_at)) = selection.merged else {
        return Err(internal("tap_merge_completed_outcome_required"));
    };
    let history = capture_all(candidate, authority, api)?;
    let selected = history
        .iter()
        .find(|pull| pull.number == selection.pull_request)
        .ok_or_else(|| internal("tap_merge_completed_pr_missing"))?;
    verify_selected(
        candidate,
        selected,
        &selection,
        expected_merge_sha,
        expected_merged_at,
    )?;
    if history.iter().filter(|pull| pull.merged).count() != 1 {
        return Err(internal("tap_merge_duplicate_completed_prs"));
    }
    let current_main = current_main_sha(candidate, api)?;
    let merge_is_ancestor = is_ancestor(candidate, api, expected_merge_sha, &current_main)?;
    let current_formula = contents(
        api,
        &candidate.policy.repository,
        &candidate.policy.formula,
        &current_main,
    )?;
    validate_formula_at_base(
        candidate,
        &current_main,
        &current_formula,
        &candidate.formula,
    )?;
    let protection = resolve_protection(
        api,
        &candidate.policy.repository,
        &candidate.plan.default_branch,
        &ProtectionAuthority {
            writer_app_id: authority.writer_app_id,
            writer_bot_user_id: authority.bot_user_id,
            writer_bot_login: &authority.bot_login,
            completion_check_name: &authority.completion_name,
            completion_app_id: authority.completion_app_id,
        },
    )?;
    let checks = exact_required_checks(&protection, &selected.check_runs, selection.head_sha)?
        .ok_or_else(|| internal("tap_merge_completed_checks_incomplete"))?;
    let observed = TapCompletedPromotionObservation {
        pull_request: selected.feedback.clone(),
        validated_base_sha: selection.base_sha.to_owned(),
        observed_pr_base_sha: selected.observed_base_sha.clone(),
        validated_head_sha: selection.head_sha.to_owned(),
        merge_sha: expected_merge_sha.to_owned(),
        main: TapCompletedMainObservation {
            current_sha: current_main,
            merge_is_ancestor,
            formula_matches_receipt: current_formula == candidate.formula,
        },
        expected_merged_pull_request: selected.merged,
        server_protection: if protection.strict && !protection.writer_bypass {
            TapServerMergeProtection::PullRequestProtected
        } else {
            TapServerMergeProtection::MissingOrBypassable
        },
        required_checks_complete: true,
        checks,
        historical_feedback_complete: true,
        historical_feedback: history
            .iter()
            .filter(|pull| pull.number != selection.pull_request)
            .map(|pull| pull.feedback.clone())
            .collect(),
    };
    validate_tap_completed_promotion_gate(&authority.policy, &observed)
        .map_err(|_| internal("tap_merge_completed_gate_rejected"))
}

fn verify_selected(
    candidate: &MergeCandidate,
    selected: &AuthenticatedPull,
    selection: &super::candidate::PromotionSelection<'_>,
    merge_sha: &str,
    merged_at: &str,
) -> Result<(), OrchestratorError> {
    if !selected.merged
        || selected.number != selection.pull_request
        || selected.head_sha != selection.head_sha
        || selected.base_sha != selection.base_sha
        || selected.observed_base_sha != selection.base_sha
        || selected.branch != selection.branch
        || selected.candidate_proof_digest != selection.candidate_proof_digest
        || selected.merge_sha.as_deref() != Some(merge_sha)
        || selected.merged_at.as_deref() != Some(merged_at)
        || selected.feedback.head_sha != selection.head_sha
        || candidate.proof.new_formula_sha256 != candidate.proof.old_formula_sha256
    {
        return Err(internal("tap_merge_completed_identity_mismatch"));
    }
    Ok(())
}

fn current_main_sha(
    candidate: &MergeCandidate,
    api: &TapMergeGitHub,
) -> Result<String, OrchestratorError> {
    let main: Value = api.get(&format!(
        "repos/{}/commits/{}",
        candidate.policy.repository,
        super::observation::encode_segment(&candidate.plan.default_branch)
    ))?;
    let sha = main["sha"]
        .as_str()
        .filter(|sha| is_lower_hex(sha, 40))
        .ok_or_else(|| internal("tap_merge_completed_main_identity"))?;
    Ok(sha.to_owned())
}

fn is_ancestor(
    candidate: &MergeCandidate,
    api: &TapMergeGitHub,
    ancestor: &str,
    descendant: &str,
) -> Result<bool, OrchestratorError> {
    let result: Value = api.get(&format!(
        "repos/{}/compare/{ancestor}...{descendant}",
        candidate.policy.repository
    ))?;
    let status = result["status"].as_str();
    let ahead = result["ahead_by"].as_u64();
    let behind = result["behind_by"].as_u64();
    if behind != Some(0) {
        return Ok(false);
    }
    Ok(matches!(status, Some("ahead" | "identical")) && ahead.is_some())
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
