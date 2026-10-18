//! Authenticated GitHub observations used for every poll and final admission.

use std::{collections::BTreeSet, env};

use base64::Engine as _;
use serde_json::Value;
use velnor_actions_contract::homebrew_merge_gate::{
    TapMergeObservation, TapMergePolicy, TapServerMergeProtection,
};

use crate::{OrchestratorError, internal::internal};

use super::{
    candidate::MergeCandidate,
    feedback::{FeedbackContext, capture_feedback},
    github::TapMergeGitHub,
    protection::{ProtectionAuthority, exact_required_checks, resolve_protection},
    tree::verify_formula_delta,
};

const PAGE_LIMIT: usize = 100;
const MAX_PAGES: u64 = 100;
use velnor_actions_contract::homebrew_review_completion::GITHUB_ACTIONS_APP_ID;

pub(super) struct MergeAuthority {
    pub(super) policy: TapMergePolicy,
    pub(super) writer_app_id: u64,
    completion_app_id: u64,
    completion_name: String,
    reviewer_ids: BTreeSet<u64>,
    bot_login: String,
    bot_user_id: u64,
    repo_id: u64,
}

pub(super) fn load_merge_authority(
    candidate: &MergeCandidate,
    api: &TapMergeGitHub,
) -> Result<MergeAuthority, OrchestratorError> {
    let writer_app_id = required_numeric("VELNOR_TAP_APP_ID")?;
    let completion_app_id = required_numeric("VELNOR_TAP_COMPLETION_APP_ID")?;
    validate_completion_app_ids(writer_app_id, completion_app_id)?;
    let reviewer_ids = candidate
        .policy
        .review_completion
        .expected_reviewers
        .iter()
        .enumerate()
        .map(|(index, _reviewer)| required_numeric(&format!("VELNOR_TAP_REVIEWER_APP_ID_{index}")))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if reviewer_ids.len() != candidate.policy.review_completion.expected_reviewers.len() {
        return Err(internal("tap_merge_reviewer_identity_duplicate"));
    }
    if reviewer_ids.contains(&completion_app_id) {
        return Err(internal("tap_merge_completion_app_not_independent"));
    }
    let app_slug = required_string("VELNOR_TAP_APP_SLUG")?;
    if !valid_app_slug(&app_slug) {
        return Err(internal("tap_merge_app_slug_invalid"));
    }
    let installation_id = required_numeric("VELNOR_TAP_INSTALLATION_ID")?;
    let app: Value = api.get(&format!("apps/{app_slug}"))?;
    if app["slug"].as_str() != Some(app_slug.as_str()) || app["id"].as_u64() != Some(writer_app_id)
    {
        return Err(internal("tap_merge_writer_app_identity"));
    }
    let repo: Value = api.get(&format!("repos/{}", candidate.policy.repository))?;
    let repo_id = repo["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("tap_merge_repository_identity"))?;
    if repo["full_name"].as_str() != Some(candidate.policy.repository.as_str()) {
        return Err(internal("tap_merge_repository_identity"));
    }
    let installation: Value = api.get("installation/repositories?per_page=100&page=1")?;
    let repositories = installation["repositories"]
        .as_array()
        .ok_or_else(|| internal("tap_merge_installation_scope"))?;
    // Installation tokens expose repository scope here; GitHub does not expose a
    // token-authenticated endpoint that returns the installation's numeric ID.
    if installation_id == 0
        || installation["total_count"].as_u64() != Some(1)
        || repositories.len() != 1
        || repositories[0]["id"].as_u64() != Some(repo_id)
    {
        return Err(internal("tap_merge_installation_scope"));
    }
    let bot_login = format!("{app_slug}[bot]");
    let bot: Value = api.get(&format!("users/{bot_login}"))?;
    let bot_user_id = bot["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .filter(|_| bot["login"].as_str() == Some(bot_login.as_str()))
        .ok_or_else(|| internal("tap_merge_writer_bot_identity"))?;
    if bot_user_id == 0 {
        return Err(internal("tap_merge_writer_bot_identity"));
    }
    let policy = TapMergePolicy {
        repository: candidate.policy.repository.clone(),
        writer_app_id,
        completion_app_id,
        completion_check_name: candidate.policy.review_completion.check_name.clone(),
        expected_reviewer_app_ids: reviewer_ids.clone(),
    };
    Ok(MergeAuthority {
        policy,
        writer_app_id,
        completion_app_id,
        completion_name: candidate.policy.review_completion.check_name.clone(),
        reviewer_ids,
        bot_login,
        bot_user_id,
        repo_id,
    })
}

fn validate_completion_app_ids(
    writer_app_id: u64,
    completion_app_id: u64,
) -> Result<(), OrchestratorError> {
    if completion_app_id == writer_app_id || completion_app_id == GITHUB_ACTIONS_APP_ID {
        return Err(internal("tap_merge_completion_app_not_independent"));
    }
    Ok(())
}

pub(super) fn observe(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    api: &TapMergeGitHub,
) -> Result<Option<TapMergeObservation>, OrchestratorError> {
    let number = candidate.promotion.pull_request;
    let (current_base, current_head) = validate_pr_state(candidate, authority, api)?;
    verify_commit(candidate, authority, api, &current_head)?;
    verify_formula_delta(candidate, api)?;
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
    let runs = check_runs(api, &candidate.policy.repository, &current_head)?;
    if let Some(checks) = exact_required_checks(&protection, &runs, &current_head)? {
        let feedback = capture_feedback(
            api,
            &FeedbackContext {
                repository: &candidate.policy.repository,
                pull_request_number: number,
                head_sha: &current_head,
                completion_app_id: authority.completion_app_id,
                completion_check_name: &authority.completion_name,
                expected_reviewer_app_ids: &authority.reviewer_ids,
            },
            &runs,
        )?;
        let observed = TapMergeObservation {
            pull_request_number: number,
            validated_base_sha: candidate.promotion.base_sha.clone(),
            current_base_sha: current_base.clone(),
            validated_head_sha: candidate.promotion.head_sha.clone(),
            current_head_sha: current_head.clone(),
            expected_open_pull_request: true,
            server_protection: if protection.strict && !protection.writer_bypass {
                TapServerMergeProtection::StrictWithoutWriterBypass
            } else {
                TapServerMergeProtection::MissingOrBypassable
            },
            required_checks_complete: true,
            checks,
            feedback: feedback.snapshot,
            completion: feedback.completion,
        };
        return Ok(Some(observed));
    }
    Ok(None)
}

fn validate_pr_state(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    api: &TapMergeGitHub,
) -> Result<(String, String), OrchestratorError> {
    let number = candidate.promotion.pull_request;
    let pr: Value = api.get(&format!(
        "repos/{}/pulls/{number}",
        candidate.policy.repository
    ))?;
    let main: Value = api.get(&format!(
        "repos/{}/commits/{}",
        candidate.policy.repository,
        encode_segment(&candidate.plan.default_branch)
    ))?;
    let current_base = main["sha"]
        .as_str()
        .ok_or_else(|| internal("tap_merge_base_missing"))?;
    let current_head = pr
        .pointer("/head/sha")
        .and_then(Value::as_str)
        .ok_or_else(|| internal("tap_merge_head_missing"))?;
    let valid = current_base == candidate.promotion.base_sha
        && current_head == candidate.promotion.head_sha
        && pr["state"].as_str() == Some("open")
        && pr["draft"].as_bool() == Some(false)
        && pr.pointer("/base/ref").and_then(Value::as_str)
            == Some(candidate.plan.default_branch.as_str())
        && pr.pointer("/base/sha").and_then(Value::as_str)
            == Some(candidate.promotion.base_sha.as_str())
        && pr.pointer("/head/ref").and_then(Value::as_str)
            == Some(candidate.promotion.branch.as_str())
        && pr.pointer("/head/repo/full_name").and_then(Value::as_str)
            == Some(candidate.policy.repository.as_str())
        && pr.pointer("/user/login").and_then(Value::as_str) == Some(authority.bot_login.as_str())
        && pr.pointer("/user/id").and_then(Value::as_u64) == Some(authority.bot_user_id)
        && pr["title"].as_str() == Some(expected_pr_title(candidate).as_str())
        && pr["body"].as_str() == Some(expected_pr_body(candidate).as_str());
    if !valid {
        return Err(internal("tap_merge_pull_request_identity_changed"));
    }
    verify_repo_id(pr.pointer("/base/repo/id"), authority.repo_id)?;
    verify_repo_id(pr.pointer("/head/repo/id"), authority.repo_id)?;
    Ok((current_base.to_owned(), current_head.to_owned()))
}

fn verify_commit(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    api: &TapMergeGitHub,
    head_sha: &str,
) -> Result<(), OrchestratorError> {
    let commit: Value = api.get(&format!(
        "repos/{}/git/commits/{head_sha}",
        candidate.policy.repository
    ))?;
    let expected_message = format!(
        "Update preview formula to {}\n\nSource: {}@{}\nRelease receipt: {}\n\nSigned-off-by: {} <{}+{}@users.noreply.github.com>",
        candidate.proof.version,
        candidate.proof.source_repository,
        candidate.proof.source_sha,
        candidate.proof.receipt_digest,
        authority.bot_login,
        authority.bot_user_id,
        authority.bot_login,
    );
    let parents = commit["parents"]
        .as_array()
        .ok_or_else(|| internal("tap_merge_commit_parent"))?;
    if commit["sha"].as_str() != Some(head_sha)
        || commit.pointer("/commit/message").and_then(Value::as_str)
            != Some(expected_message.as_str())
        || parents.len() != 1
        || parents[0]["sha"].as_str() != Some(candidate.promotion.base_sha.as_str())
    {
        return Err(internal("tap_merge_commit_identity_mismatch"));
    }
    Ok(())
}

fn expected_pr_title(candidate: &MergeCandidate) -> String {
    format!(
        "Update preview formula to {}",
        candidate.release.receipt.version.value
    )
}

fn expected_pr_body(candidate: &MergeCandidate) -> String {
    format!(
        "Promotes immutable preview release `{}` from `{}` at `{}`.\n\nReceipt digest: `{}`\nCandidate proof digest: `{}`\nTap base: `{}`.",
        candidate.release.receipt.tag,
        candidate.release.receipt.repository.slug,
        candidate.release.receipt.source.commit_sha,
        candidate.release.receipt_digest,
        candidate.promotion.candidate_proof_digest,
        candidate.promotion.base_sha,
    )
}

pub(super) fn contents(
    api: &TapMergeGitHub,
    repository: &str,
    path: &str,
    reference: &str,
) -> Result<Vec<u8>, OrchestratorError> {
    let route = path
        .split('/')
        .map(encode_segment)
        .collect::<Vec<_>>()
        .join("/");
    let value: Value = api.get(&format!(
        "repos/{repository}/contents/{route}?ref={reference}"
    ))?;
    if value["encoding"].as_str() != Some("base64") {
        return Err(internal("tap_merge_formula_encoding"));
    }
    base64::engine::general_purpose::STANDARD
        .decode(
            value["content"]
                .as_str()
                .ok_or_else(|| internal("tap_merge_formula_content"))?
                .replace('\n', ""),
        )
        .map_err(|_| internal("tap_merge_formula_base64"))
}

fn check_runs(
    api: &TapMergeGitHub,
    repository: &str,
    sha: &str,
) -> Result<Vec<Value>, OrchestratorError> {
    let mut all = Vec::new();
    for page in 1..=MAX_PAGES {
        let response: Value = api.get(&format!(
            "repos/{repository}/commits/{sha}/check-runs?per_page={PAGE_LIMIT}&page={page}"
        ))?;
        let runs = response["check_runs"]
            .as_array()
            .ok_or_else(|| internal("tap_merge_check_runs_invalid"))?;
        if runs.len() > PAGE_LIMIT || all.len().saturating_add(runs.len()) > 20_000 {
            return Err(internal("tap_merge_check_page_limit"));
        }
        let count = runs.len();
        all.extend(runs.iter().cloned());
        if count < PAGE_LIMIT {
            return Ok(all);
        }
    }
    Err(internal("tap_merge_check_page_limit"))
}

fn required_numeric(name: &str) -> Result<u64, OrchestratorError> {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("tap_merge_numeric_identity_missing"))
}

fn required_string(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .ok_or_else(|| internal("tap_merge_identity_missing"))
}

fn valid_app_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 39
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
}

fn verify_repo_id(value: Option<&Value>, expected: u64) -> Result<(), OrchestratorError> {
    if value.and_then(Value::as_u64) == Some(expected) {
        Ok(())
    } else {
        Err(internal("tap_merge_target_repository_mismatch"))
    }
}

fn encode_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::validate_completion_app_ids;

    #[test]
    fn completion_app_must_be_separate_from_writer_and_github_actions() {
        assert!(validate_completion_app_ids(17, 19).is_ok());
        assert!(validate_completion_app_ids(17, 17).is_err());
        assert!(validate_completion_app_ids(17, 15_368).is_err());
    }
}
