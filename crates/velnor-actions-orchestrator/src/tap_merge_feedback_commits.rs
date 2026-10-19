//! Complete PR commit inventory; native writer single-commit policy is separate.

use super::{
    FeedbackContext, MAX_FEEDBACK_RECORDS, TapMergeGitHub, Value, internal, paged_commit_comments,
    paged_values,
};
use crate::OrchestratorError;
use std::collections::BTreeSet;

pub(super) fn inventory(
    api: &TapMergeGitHub,
    context: &FeedbackContext<'_>,
) -> Result<Vec<String>, OrchestratorError> {
    let pull: Value = api.get(&format!(
        "repos/{}/pulls/{}",
        context.repository, context.pull_request_number
    ))?;
    let maximum =
        u64::try_from(MAX_FEEDBACK_RECORDS).map_err(|_| internal("tap_feedback_commit_count"))?;
    let count = pull["commits"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= maximum)
        .ok_or_else(|| internal("tap_feedback_commit_count"))?;
    validate_head(
        &pull,
        context,
        usize::try_from(count).map_err(|_| internal("tap_feedback_commit_count"))?,
    )?;
    let rows = paged_values(
        api,
        &format!(
            "repos/{}/pulls/{}/commits",
            context.repository, context.pull_request_number
        ),
    )?;
    if u64::try_from(rows.len()).ok() != Some(count) {
        return Err(internal("tap_feedback_commit_inventory"));
    }
    let mut seen = BTreeSet::new();
    let mut commits = Vec::new();
    for row in rows {
        let sha = row["sha"]
            .as_str()
            .filter(|sha| valid_sha(sha))
            .ok_or_else(|| internal("tap_feedback_commit_sha"))?
            .to_owned();
        if !seen.insert(sha.clone()) {
            return Err(internal("tap_feedback_commit_duplicate"));
        }
        commits.push(sha);
    }
    if commits.last().map(String::as_str) != Some(context.head_sha) {
        return Err(internal("tap_feedback_commit_head"));
    }
    Ok(commits)
}

pub(super) fn comments(
    api: &TapMergeGitHub,
    repository: &str,
    commits: &[String],
) -> Result<Vec<Value>, OrchestratorError> {
    let mut all = Vec::new();
    let mut ids = BTreeSet::new();
    for commit in commits {
        for comment in paged_commit_comments(api, repository, commit)? {
            if all.len() >= MAX_FEEDBACK_RECORDS
                || !ids.insert(
                    comment["id"]
                        .as_u64()
                        .ok_or_else(|| internal("tap_merge_commit_comment_identity"))?,
                )
            {
                return Err(internal("tap_feedback_commit_comment_inventory"));
            }
            all.push(comment);
        }
    }
    Ok(all)
}

pub(super) fn require_head(
    api: &TapMergeGitHub,
    context: &FeedbackContext<'_>,
    count: usize,
) -> Result<(), OrchestratorError> {
    let pull: Value = api.get(&format!(
        "repos/{}/pulls/{}",
        context.repository, context.pull_request_number
    ))?;
    validate_head(&pull, context, count)
}

fn validate_head(
    pull: &Value,
    context: &FeedbackContext<'_>,
    count: usize,
) -> Result<(), OrchestratorError> {
    if pull["number"].as_u64() != Some(context.pull_request_number)
        || pull.pointer("/head/sha").and_then(Value::as_str) != Some(context.head_sha)
        || pull["commits"].as_u64() != u64::try_from(count).ok()
    {
        return Err(internal("tap_feedback_pull_changed"));
    }
    Ok(())
}

fn valid_sha(sha: &str) -> bool {
    sha.len() == 40
        && sha
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}

#[cfg(test)]
#[path = "tap_merge_feedback_commits_tests.rs"]
mod tests;
