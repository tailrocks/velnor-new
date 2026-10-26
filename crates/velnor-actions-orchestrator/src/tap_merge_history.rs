#[path = "tap_merge_history_identity.rs"]
mod identity;
#[path = "tap_merge_history_landed.rs"]
mod landed;

use serde_json::Value;
use velnor_actions_contract::{canonical_json_bytes, homebrew_merge_gate::TapHistoricalFeedback};

use crate::{OrchestratorError, internal::internal};

use super::{
    candidate::MergeCandidate,
    feedback::{FeedbackContext, capture_feedback},
    github::TapMergeGitHub,
    observation::MergeAuthority,
    observation::{check_runs, contents, expected_pr_body_for, expected_pr_title},
    tree::verify_formula_delta_at_base,
};
use identity::{
    belongs_to_receipt, branch_base, branch_prefix, required_sha, required_text, verify_repo_id,
};

const PAGE_SIZE: usize = 100;
const MAX_PAGES: u64 = 100;
const MAX_PULLS: usize = 10_000;

pub(super) struct AuthenticatedPull {
    pub(super) number: u64,
    pub(super) head_sha: String,
    pub(super) base_sha: String,
    pub(super) observed_base_sha: String,
    pub(super) branch: String,
    pub(super) merged: bool,
    pub(super) merge_sha: Option<String>,
    pub(super) merged_at: Option<String>,
    pub(super) candidate_proof_digest: String,
    pub(super) feedback: TapHistoricalFeedback,
    pub(super) check_runs: Vec<Value>,
}

struct HistoricalFormula {
    base_sha: String,
    old_formula_sha256: String,
    head_bytes: Vec<u8>,
    proof_digest: String,
}

struct MergeProjection<'a> {
    number: u64,
    branch: &'a str,
    head_sha: &'a str,
    observed_base_sha: &'a str,
    merged: bool,
    merged_at: Option<&'a str>,
}

pub(super) fn capture_all(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    api: &TapMergeGitHub,
) -> Result<Vec<AuthenticatedPull>, OrchestratorError> {
    let summaries = list_receipt_prs(candidate, api)?;
    let mut authenticated = Vec::with_capacity(summaries.len());
    for summary in summaries {
        authenticated.push(authenticate_pull(candidate, authority, api, &summary)?);
    }
    authenticated.sort_by_key(|pull| pull.number);
    if authenticated
        .windows(2)
        .any(|pair| pair[0].number == pair[1].number)
    {
        return Err(internal("tap_merge_history_duplicate_pr"));
    }
    Ok(authenticated)
}

fn list_receipt_prs(
    candidate: &MergeCandidate,
    api: &TapMergeGitHub,
) -> Result<Vec<Value>, OrchestratorError> {
    let digest_marker = format!("Receipt digest: `{}`", candidate.release.receipt_digest);
    let receipt_prefix = branch_prefix(
        &candidate.release.receipt_digest,
        candidate.release.receipt.channel,
    )?;
    let mut selected = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for page in 1..=MAX_PAGES {
        let response: Vec<Value> = api.get(&format!(
            "repos/{}/pulls?state=all&per_page={PAGE_SIZE}&page={page}",
            candidate.policy.repository
        ))?;
        if response.len() > PAGE_SIZE || selected.len().saturating_add(response.len()) > MAX_PULLS {
            return Err(internal("tap_merge_history_page_limit"));
        }
        for pull in &response {
            let number = pull["number"]
                .as_u64()
                .filter(|number| *number > 0)
                .ok_or_else(|| internal("tap_merge_history_pr_number"))?;
            if !seen.insert(number) {
                return Err(internal("tap_merge_history_duplicate_pr"));
            }
            let branch = pull
                .pointer("/head/ref")
                .and_then(Value::as_str)
                .ok_or_else(|| internal("tap_merge_history_branch"))?;
            let body = pull["body"].as_str().unwrap_or_default();
            if belongs_to_receipt(branch, body, &receipt_prefix, &digest_marker)? {
                selected.push(pull.clone());
            }
        }
        let page_len = response.len();
        if page_len < PAGE_SIZE {
            return Ok(selected);
        }
    }
    Err(internal("tap_merge_history_page_limit"))
}

fn authenticate_pull(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    api: &TapMergeGitHub,
    summary: &Value,
) -> Result<AuthenticatedPull, OrchestratorError> {
    let (number, detail) = read_pull_detail(candidate, api, summary)?;
    let branch = required_text(&detail, "/head/ref", "tap_merge_history_branch")?.to_owned();
    let head_sha = required_sha(&detail, "/head/sha", "tap_merge_history_head")?;
    validate_pull_identity(candidate, authority, &detail, number, &branch, &head_sha)?;
    let base_sha = branch_base(
        &candidate.release.receipt_digest,
        &branch,
        candidate.release.receipt.channel,
    )?;
    let observed_base_sha = required_sha(&detail, "/base/sha", "tap_merge_history_base")?;
    let (merged, merged_at) = merged_fields(&detail)?;
    let merge_sha = landed::read_merge_sha(
        candidate,
        authority,
        api,
        &detail,
        &MergeProjection {
            number,
            branch: &branch,
            head_sha: &head_sha,
            observed_base_sha: &observed_base_sha,
            merged,
            merged_at: merged_at.as_deref(),
        },
    )?;
    identity::validate_observed_base(&detail, &base_sha, merged)?;
    let formula = validate_historical_formula(candidate, api, number, &detail, &branch, &head_sha)?;
    verify_commit(
        candidate,
        authority,
        api,
        number,
        &head_sha,
        &formula.base_sha,
    )?;
    verify_formula_delta_at_base(
        candidate,
        api,
        &formula.base_sha,
        &head_sha,
        &formula.head_bytes,
        &formula.old_formula_sha256,
    )?;
    let runs = check_runs(api, &candidate.policy.repository, &head_sha)?;
    let feedback = capture_feedback(
        api,
        &FeedbackContext {
            repository: &candidate.policy.repository,
            pull_request_number: number,
            head_sha: &head_sha,
            completion_app_id: authority.completion_app_id,
            completion_check_name: &authority.completion_name,
            expected_reviewer_app_ids: &authority.reviewer_ids,
        },
        &runs,
    )?;
    Ok(AuthenticatedPull {
        number,
        head_sha: head_sha.clone(),
        base_sha: formula.base_sha,
        observed_base_sha,
        branch,
        merged,
        merge_sha,
        merged_at,
        candidate_proof_digest: formula.proof_digest,
        feedback: TapHistoricalFeedback {
            pull_request_number: number,
            head_sha,
            feedback: feedback.snapshot,
            completion: feedback.completion,
        },
        check_runs: runs,
    })
}

fn read_pull_detail(
    candidate: &MergeCandidate,
    api: &TapMergeGitHub,
    summary: &Value,
) -> Result<(u64, Value), OrchestratorError> {
    let number = summary["number"]
        .as_u64()
        .filter(|number| *number > 0)
        .ok_or_else(|| internal("tap_merge_history_pr_number"))?;
    let detail = api.get(&format!(
        "repos/{}/pulls/{number}",
        candidate.policy.repository
    ))?;
    Ok((number, detail))
}

fn validate_historical_formula(
    candidate: &MergeCandidate,
    api: &TapMergeGitHub,
    number: u64,
    detail: &Value,
    branch: &str,
    head_sha: &str,
) -> Result<HistoricalFormula, OrchestratorError> {
    let base_sha = branch_base(
        &candidate.release.receipt_digest,
        branch,
        candidate.release.receipt.channel,
    )?;
    let base_formula = contents(
        api,
        &candidate.policy.repository,
        &candidate.formula_path,
        &base_sha,
    )?;
    let head_formula = contents(
        api,
        &candidate.policy.repository,
        &candidate.formula_path,
        head_sha,
    )?;
    let historical_proof =
        reconstruct_historical_proof(crate::tap_prepare::HistoricalCandidateInput {
            source_repository: &candidate.proof.source_repository,
            tap: &candidate.policy,
            plan: &candidate.plan,
            release: &candidate.release,
            spec: candidate.spec.as_ref(),
            sdk_artifact_digest: &candidate.sdk_artifact_digest,
            base_sha: &base_sha,
            base_formula: &base_formula,
            proposed_formula: &head_formula,
        })?;
    let proof_digest = sha256(
        &canonical_json_bytes(&historical_proof)
            .map_err(|_| internal("tap_merge_history_proof_encode"))?,
    );
    if detail["title"].as_str() != Some(expected_pr_title(candidate).as_str())
        || detail["body"].as_str()
            != Some(expected_pr_body_for(candidate, &base_sha, &proof_digest).as_str())
    {
        return Err(internal("tap_merge_history_pr_message_mismatch"));
    }
    if let Some(selected) = candidate.selection_for(number)
        && (selected.base_sha != base_sha
            || selected.head_sha != head_sha
            || selected.candidate_proof_digest != proof_digest)
    {
        return Err(internal("tap_merge_history_selected_binding"));
    }
    Ok(HistoricalFormula {
        base_sha,
        old_formula_sha256: historical_proof.old_formula_sha256,
        head_bytes: head_formula,
        proof_digest,
    })
}

fn merged_fields(detail: &Value) -> Result<(bool, Option<String>), OrchestratorError> {
    let merged = detail["merged"]
        .as_bool()
        .ok_or_else(|| internal("tap_merge_history_merged_missing"))?;
    if merged {
        if detail["state"].as_str() != Some("closed") {
            return Err(internal("tap_merge_history_merged_state"));
        }
        let at = required_text(detail, "/merged_at", "tap_merge_history_merged_at")?;
        Ok((true, Some(at.to_owned())))
    } else {
        Ok((false, None))
    }
}

fn reconstruct_historical_proof(
    input: crate::tap_prepare::HistoricalCandidateInput<'_>,
) -> Result<crate::tap_prepare::HomebrewTapPreparationProof, OrchestratorError> {
    crate::tap_prepare::reconstruct_historical_candidate_proof(input)
}

fn validate_pull_identity(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    pull: &Value,
    number: u64,
    branch: &str,
    head_sha: &str,
) -> Result<(), OrchestratorError> {
    let owner = candidate
        .policy
        .repository
        .split_once('/')
        .map(|(owner, _)| owner)
        .ok_or_else(|| internal("tap_merge_repository_invalid"))?;
    if pull["number"].as_u64() != Some(number)
        || !matches!(pull["state"].as_str(), Some("open" | "closed"))
        || pull.pointer("/base/ref").and_then(Value::as_str)
            != Some(candidate.plan.default_branch.as_str())
        || pull.pointer("/head/ref").and_then(Value::as_str) != Some(branch)
        || pull.pointer("/head/sha").and_then(Value::as_str) != Some(head_sha)
        || pull.pointer("/base/repo/full_name").and_then(Value::as_str)
            != Some(candidate.policy.repository.as_str())
        || pull.pointer("/head/repo/full_name").and_then(Value::as_str)
            != Some(candidate.policy.repository.as_str())
        || pull.pointer("/head/label").and_then(Value::as_str)
            != Some(format!("{owner}:{branch}").as_str())
        || pull.pointer("/user/login").and_then(Value::as_str) != Some(authority.bot_login.as_str())
        || pull.pointer("/user/id").and_then(Value::as_u64) != Some(authority.bot_user_id)
    {
        return Err(internal("tap_merge_history_pr_identity"));
    }
    verify_repo_id(pull.pointer("/base/repo/id"), authority.repo_id)?;
    verify_repo_id(pull.pointer("/head/repo/id"), authority.repo_id)?;
    Ok(())
}

fn verify_commit(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    api: &TapMergeGitHub,
    number: u64,
    head_sha: &str,
    base_sha: &str,
) -> Result<(), OrchestratorError> {
    let commit: Value = api.get(&format!(
        "repos/{}/git/commits/{head_sha}",
        candidate.policy.repository
    ))?;
    let message = format!(
        "Update {} formula to {}\n\nSource: {}@{}\nRelease receipt: {}\n\nSigned-off-by: {} <{}+{}@users.noreply.github.com>",
        match candidate.release.receipt.channel {
            velnor_actions_contract::package_release::PackageReleaseChannel::Preview => "preview",
            velnor_actions_contract::package_release::PackageReleaseChannel::Stable => "stable",
        },
        candidate.release.receipt.version.value,
        candidate.proof.source_repository,
        candidate.proof.source_sha,
        candidate.release.receipt_digest,
        authority.bot_login,
        authority.bot_user_id,
        authority.bot_login,
    );
    if number == 0 || !identity::commit_matches(&commit, head_sha, base_sha, &message) {
        return Err(internal("tap_merge_history_commit_identity"));
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
            out
        })
}
