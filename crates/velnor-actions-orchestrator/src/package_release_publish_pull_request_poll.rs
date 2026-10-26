// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;
use velnor_actions_mise::CheckDeadline;

use super::{GitHubApi, OrchestratorError, PublicPullRequestResponse, github, internal};

const MAX_ATTEMPTS: usize = 10;

pub(super) fn read(
    api: &GitHubApi,
    repository: &str,
    repository_id: u64,
    number: u64,
    deadline: CheckDeadline,
) -> Result<PublicPullRequestResponse, OrchestratorError> {
    let route = api.route(&format!("pulls/{number}"));
    let first: PublicPullRequestResponse = api.get_json(&route)?;
    validate_identity(&first, repository, repository_id, number)?;
    if first.state != "open" || first.merged_at.is_some() || first.mergeable.is_some() {
        return Ok(first);
    }
    let mut delay = Duration::from_millis(250);
    for _ in 1..MAX_ATTEMPTS {
        let remaining = deadline
            .remaining()
            .map_err(|_| internal("package_public_verification_deadline"))?;
        std::thread::sleep(delay.min(remaining));
        deadline
            .remaining()
            .map_err(|_| internal("package_public_verification_deadline"))?;
        let next: PublicPullRequestResponse = api.get_json(&route)?;
        deadline
            .remaining()
            .map_err(|_| internal("package_public_verification_deadline"))?;
        if first.number != next.number
            || first.state != next.state
            || first.draft != next.draft
            || first.merged_at != next.merged_at
            || first.base != next.base
            || first.head != next.head
        {
            return Err(internal("package_public_pull_request_changed"));
        }
        if next.mergeable.is_some() {
            return Ok(next);
        }
        delay = (delay * 2).min(Duration::from_secs(2));
    }
    Err(internal(
        "package_public_pull_request_mergeability_unresolved",
    ))
}

fn validate_identity(
    response: &PublicPullRequestResponse,
    repository: &str,
    repository_id: u64,
    number: u64,
) -> Result<(), OrchestratorError> {
    let base = &response.base;
    let head = &response.head;
    let (Some(base_repository), Some(head_repository)) = (&base.repo, &head.repo) else {
        return Err(internal("package_public_pull_request_repository_missing"));
    };
    if response.number != number
        || base_repository.id != repository_id
        || head_repository.id == 0
        || base_repository.full_name != repository
        || !velnor_actions_contract::vcs::is_safe_github_repository(&head_repository.full_name)
        || !github::is_lowercase_git_sha(&base.sha)
        || !github::is_lowercase_git_sha(&head.sha)
        || base.reference.is_empty()
        || head.reference.is_empty()
        || base.reference.bytes().any(|byte| byte.is_ascii_control())
        || head.reference.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(internal("package_public_pull_request_identity_mismatch"));
    }
    Ok(())
}
