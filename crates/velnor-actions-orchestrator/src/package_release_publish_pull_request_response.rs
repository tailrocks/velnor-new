// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_mise::CheckDeadline;

use super::{
    GitHubApi, OrchestratorError, PublicPullRequestResponse, PublicPullRequestSnapshot, internal,
    public_pull_request_snapshot, pull_request_poll,
};

pub(crate) fn parse_public_pull_request_response(
    bytes: &[u8],
    repository: &str,
    repository_id: u64,
    number: u64,
) -> Result<PublicPullRequestSnapshot, OrchestratorError> {
    let response: PublicPullRequestResponse =
        serde_json::from_slice(bytes).map_err(|_| internal("package_public_pull_request_json"))?;
    pull_request_poll::validate_identity(&response, repository, repository_id, number)?;
    public_pull_request_snapshot(response, repository, repository_id, None)
}

pub(crate) fn read_public_merge_group_pull_request_response(
    repository: &str,
    repository_id: u64,
    number: u64,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(repository)
        || repository_id == 0
        || number == 0
    {
        return Err(internal("package_public_pull_request_selection_invalid"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let api = GitHubApi::new(repository.to_owned()).with_deadline(deadline);
    api.verify_repository_identity(repository_id)?;
    let bytes = api.get_json_bytes(&api.route(&format!("pulls/{number}")))?;
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    Ok(bytes)
}
