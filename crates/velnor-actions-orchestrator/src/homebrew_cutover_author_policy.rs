// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::{OrchestratorError, internal::internal};

const POLICY_LIMIT: usize = 4096;
const POLICY: &[u8] =
    include_bytes!("../../../.velnor/qualification/homebrew/jackin/cutover-author-policy.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CutoverAuthorPolicy {
    schema: String,
    transition_id: String,
    transition_digest: String,
    tap_repository: String,
    tap_repository_id: u64,
    author: CutoverAuthor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CutoverAuthor {
    id: u64,
    login: String,
    #[serde(rename = "type")]
    account_type: String,
}

pub(crate) fn verify_transition(
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
) -> Result<crate::github_landed_pull::GitHubRestUser, OrchestratorError> {
    parse_policy(POLICY, transition)
}

fn parse_policy(
    bytes: &[u8],
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
) -> Result<crate::github_landed_pull::GitHubRestUser, OrchestratorError> {
    let invalid = || internal("homebrew_cutover_author_policy_invalid");
    if bytes.is_empty() || bytes.len() > POLICY_LIMIT {
        return Err(invalid());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let value = velnor_actions_contract::parse_strict_json(text).map_err(|_| invalid())?;
    let policy: CutoverAuthorPolicy = serde_json::from_value(value).map_err(|_| invalid())?;
    if velnor_actions_contract::canonical_json_bytes(&policy).map_err(|_| invalid())? != bytes
        || policy.schema != "velnor.homebrew-cutover-author-policy.v1"
        || policy.transition_id != transition.transition_id
        || policy.transition_digest
            != transition
                .digest()
                .map_err(crate::internal::internal_contract)?
        || policy.tap_repository != transition.tap_repository
        || policy.tap_repository_id != transition.tap_repository_id
        || policy.author.id == 0
        || policy.author.account_type != "User"
        || !crate::github_landed_pull::GitHubRestUser::valid_login(&policy.author.login)
    {
        return Err(invalid());
    }
    Ok(crate::github_landed_pull::GitHubRestUser {
        id: policy.author.id,
        login: policy.author.login,
        account_type: policy.author.account_type,
    })
}

pub(crate) fn require_author(
    expected: &crate::github_landed_pull::GitHubRestUser,
    observed: &crate::github_landed_pull::GitHubRestUser,
) -> Result<(), OrchestratorError> {
    if expected != observed {
        return Err(internal("homebrew_cutover_author_mismatch"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "homebrew_cutover_author_policy_tests.rs"]
mod tests;
