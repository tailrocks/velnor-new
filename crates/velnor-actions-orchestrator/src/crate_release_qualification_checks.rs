// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, crate_release_github::GitHubAdmissionRead, internal::internal};
use serde_json::Value;
use std::collections::BTreeSet;

pub(crate) fn complete_check_runs(
    api: &dyn GitHubAdmissionRead,
    repository: &str,
    sha: &str,
) -> Result<Vec<Value>, OrchestratorError> {
    let invalid = || internal("qualification_check_inventory_invalid");
    if !velnor_actions_contract::vcs::is_safe_github_repository(repository)
        || sha.len() != 40
        || !sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let mut values = Vec::new();
    let mut ids = BTreeSet::new();
    let mut expected = None;
    for page in 1..=100 {
        let result = api.get(&format!(
            "repos/{repository}/commits/{sha}/check-runs?filter=all&per_page=100&page={page}"
        ))?;
        let total = result["total_count"]
            .as_u64()
            .filter(|v| *v <= 10_000)
            .ok_or_else(invalid)?;
        let total_usize = usize::try_from(total).map_err(|_| invalid())?;
        if expected.is_some_and(|previous| previous != total) {
            return Err(invalid());
        }
        expected = Some(total);
        let checks = result["check_runs"].as_array().ok_or_else(invalid)?;
        if checks.len() > 100 || (checks.is_empty() && values.len() < total_usize) {
            return Err(invalid());
        }
        for check in checks {
            let id = check["id"]
                .as_u64()
                .filter(|id| *id > 0)
                .ok_or_else(invalid)?;
            if !ids.insert(id) || check["head_sha"].as_str() != Some(sha) {
                return Err(invalid());
            }
            values.push(check.clone());
        }
        if values.len() == total_usize {
            return Ok(values);
        }
        if values.len() > total_usize {
            return Err(invalid());
        }
    }
    Err(invalid())
}

pub fn verify_refreshed_feedback(
    github: &dyn GitHubAdmissionRead,
    feedback: &dyn crate::github_review_feedback::FeedbackRead,
    context: &crate::github_review_feedback::FeedbackContext<'_>,
) -> Result<velnor_actions_contract::homebrew_merge_gate::TapFeedbackSnapshot, OrchestratorError> {
    let checks = complete_check_runs(github, context.repository, context.head_sha)?;
    let first =
        crate::github_review_feedback::verify_complete_feedback(feedback, context, &checks)?;
    let refreshed = complete_check_runs(github, context.repository, context.head_sha)?;
    if checks != refreshed {
        return Err(internal("qualification_check_inventory_changed"));
    }
    let second =
        crate::github_review_feedback::verify_complete_feedback(feedback, context, &refreshed)?;
    if first != second
        || complete_check_runs(github, context.repository, context.head_sha)? != checks
    {
        return Err(internal("qualification_feedback_changed"));
    }
    Ok(first)
}

#[cfg(test)]
#[path = "crate_release_qualification_checks_tests.rs"]
mod tests;
