//! Bounded canonical proposal data; no proposal is remote completion authority.
use super::super::{
    feedback::{CapturedFeedback, FeedbackContext},
    github::TapMergeGitHub,
};
use crate::{
    OrchestratorError, internal::internal, retrieve_reports::staged_reads::read_staged_bytes_until,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    env,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use velnor_actions_contract::{
    canonical_json_bytes, digest_b3,
    review_completion::{ReviewCompletionPolicy, parse_review_completion_policy},
};
use velnor_actions_mise::CheckDeadline;

pub(super) const PROVIDER_APP: u64 = 1_144_995;
pub(super) const PROVIDER_OWNER: u64 = 14_957_082;
pub(super) const PROVIDER_BOT: u64 = 199_175_422;
pub(super) const PROPOSAL_NAME: &str = "review-completion-proposal.json";
const MAX_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CompletionState {
    Pending,
    Blocked,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Proposal {
    pub schema: String,
    pub policy_digest: String,
    pub repository: String,
    pub repository_id: u64,
    pub pull_request_number: u64,
    pub head_sha: String,
    pub completion_app_id: u64,
    pub reviewer_app_id: u64,
    pub feedback_digest: Option<String>,
    pub provider_digest: Option<String>,
    pub state: CompletionState,
}

pub(super) struct Operation {
    pub policy: ReviewCompletionPolicy,
    pub temp: PathBuf,
    pub app_id: u64,
    pub repository_id: u64,
    pub api: TapMergeGitHub,
    pub deadline: CheckDeadline,
}

pub(super) fn operation(job: &str) -> Result<Operation, OrchestratorError> {
    if env_string("GITHUB_JOB")? != job || numeric("VELNOR_REVIEWER_APP_ID")? != PROVIDER_APP {
        return Err(internal("review_completion_role"));
    }
    let app_id = numeric("VELNOR_COMPLETION_APP_ID")?;
    if app_id == PROVIDER_APP
        || app_id == velnor_actions_contract::homebrew_review_completion::GITHUB_ACTIONS_APP_ID
    {
        return Err(internal("review_completion_app_role"));
    }
    let deadline = CheckDeadline::from_start_with_cleanup_reserve(
        Instant::now(),
        Duration::from_secs(u64::from(velnor_actions_contract::JobTimeout::DEFAULT_MINUTES) * 60),
    )
    .map_err(|_| internal("review_completion_deadline"))?;
    let temp = PathBuf::from(env_string("RUNNER_TEMP")?)
        .canonicalize()
        .map_err(|_| internal("review_completion_temp"))?
        .join("velnor");
    let path = temp.join("review-completion-policy.json");
    if Path::new(&env_string("VELNOR_REQUEST_FILE")?) != path {
        return Err(internal("review_completion_policy_path"));
    }
    let bytes = read(&path, 16 * 1024, deadline)?;
    let policy =
        parse_review_completion_policy(&bytes).map_err(|_| internal("review_completion_policy"))?;
    let reference = format!("refs/heads/{}", policy.default_branch);
    let workflow = format!(
        "{}/{}@{reference}",
        policy.repository,
        velnor_actions_contract::review_completion::REVIEW_COMPLETION_WORKFLOW_PATH
    );
    if env_string("GITHUB_WORKFLOW_REF")? != workflow || env_string("GITHUB_REF")? != reference {
        return Err(internal("review_completion_workflow_authority"));
    }
    if env_string("GITHUB_REPOSITORY")? != policy.repository {
        return Err(internal("review_completion_repository"));
    }
    let api = TapMergeGitHub::with_deadline(
        env_string("GH_TOKEN")?,
        Instant::now()
            + deadline
                .remaining()
                .map_err(|_| internal("review_completion_deadline"))?,
    );
    Ok(Operation {
        policy,
        temp,
        app_id,
        repository_id: numeric("GITHUB_REPOSITORY_ID")?,
        api,
        deadline,
    })
}

pub(super) fn read(
    path: &Path,
    cap: u64,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    read_staged_bytes_until(path, cap, || {
        deadline
            .remaining()
            .map(|_| ())
            .map_err(|_| "review_completion_deadline")
    })
    .map_err(|_| internal("review_completion_read"))
}

pub(super) fn read_proposal(
    operation: &Operation,
    expected: &str,
) -> Result<Proposal, OrchestratorError> {
    let bytes = read(
        &operation.temp.join(PROPOSAL_NAME),
        MAX_BYTES,
        operation.deadline,
    )?;
    if digest_b3(&bytes) != expected {
        return Err(internal("review_completion_proposal_digest"));
    }
    let value = velnor_actions_contract::strict_json::parse_strict_json_bytes(
        &bytes,
        usize::try_from(MAX_BYTES).map_err(|_| internal("review_completion_limit"))?,
    )
    .map_err(|_| internal("review_completion_proposal_json"))?;
    let proposal: Proposal =
        serde_json::from_value(value).map_err(|_| internal("review_completion_proposal"))?;
    if canonical_json_bytes(&proposal).map_err(|_| internal("review_completion_encode"))? != bytes
        || proposal.schema != "velnor.review-completion-proposal.v1"
        || proposal.policy_digest
            != digest_b3(
                &operation
                    .policy
                    .canonical_bytes()
                    .map_err(|_| internal("review_completion_policy"))?,
            )
        || proposal.repository != operation.policy.repository
        || proposal.repository_id != operation.repository_id
        || proposal.completion_app_id != operation.app_id
        || proposal.reviewer_app_id != PROVIDER_APP
        || proposal.pull_request_number == 0
        || !valid_sha(&proposal.head_sha)
    {
        return Err(internal("review_completion_proposal_binding"));
    }
    Ok(proposal)
}

pub(super) fn feedback(
    operation: &Operation,
    number: u64,
    head: &str,
) -> Result<CapturedFeedback, OrchestratorError> {
    let reviewers = BTreeSet::from([PROVIDER_APP]);
    super::super::feedback::capture_feedback(
        &operation.api,
        &FeedbackContext {
            repository: &operation.policy.repository,
            pull_request_number: number,
            head_sha: head,
            completion_app_id: operation.app_id,
            completion_check_name: &operation.policy.check_name,
            expected_reviewer_app_ids: &reviewers,
        },
        &[],
    )
}

pub(super) fn pull(operation: &Operation, number: u64) -> Result<Value, OrchestratorError> {
    let repository: Value = operation
        .api
        .get(&format!("repos/{}", operation.policy.repository))?;
    let pull: Value = operation.api.get(&format!(
        "repos/{}/pulls/{number}",
        operation.policy.repository
    ))?;
    if repository["id"].as_u64() != Some(operation.repository_id)
        || repository["full_name"].as_str() != Some(operation.policy.repository.as_str())
        || repository["default_branch"].as_str() != Some(operation.policy.default_branch.as_str())
        || pull["number"].as_u64() != Some(number)
        || pull.pointer("/base/repo/id").and_then(Value::as_u64) != repository["id"].as_u64()
        || pull.pointer("/base/ref").and_then(Value::as_str)
            != Some(operation.policy.default_branch.as_str())
        || !pull
            .pointer("/head/sha")
            .and_then(Value::as_str)
            .is_some_and(valid_sha)
    {
        return Err(internal("review_completion_pull_identity"));
    }
    Ok(pull)
}

pub(super) fn env_string(key: &str) -> Result<String, OrchestratorError> {
    env::var(key)
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| internal("review_completion_environment"))
}
pub(super) fn numeric(key: &str) -> Result<u64, OrchestratorError> {
    let s = env_string(key)?;
    let n = s
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| internal("review_completion_number"))?;
    if n.to_string() != s {
        return Err(internal("review_completion_number"));
    }
    Ok(n)
}
pub(super) fn valid_sha(s: &str) -> bool {
    s.len() == 40
        && s.bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}
