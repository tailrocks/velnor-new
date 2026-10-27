// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

pub(super) fn verify_cutover_author(
    root: &Path,
    identity: &crate::internal_plan::native_homebrew_checks::NativeCheckIdentityExtension,
    event: velnor_actions_contract::WorkflowEvent,
    merge_group: Option<&crate::package_release_publish::MergeGroupSelection>,
    deadline: CheckDeadline,
    read_merge_group_responses: impl FnOnce(
        &crate::package_release_publish::MergeGroupSelection,
        CheckDeadline,
    ) -> Result<(Vec<u8>, Vec<u8>), OrchestratorError>,
) -> Result<(), OrchestratorError> {
    let transition = crate::homebrew::structural_cutover::protected_transition()?;
    let expected =
        crate::homebrew::structural_cutover::verify_author_policy(transition.transition())?;
    if event == velnor_actions_contract::WorkflowEvent::MergeGroup {
        let selection =
            merge_group.ok_or_else(|| internal("homebrew_authority_merge_group_missing"))?;
        crate::homebrew::authority::verify_merge_group_checkout(
            root,
            selection,
            &transition.transition().tap_repository,
            transition.transition().tap_repository_id,
            &identity.tap_base_sha,
            &identity.head,
            deadline,
        )?;
        let (queue_response, pull_response) = read_merge_group_responses(selection, deadline)?;
        return verify_cutover_author_merge_group_responses(
            identity,
            selection,
            &queue_response,
            &pull_response,
            &expected,
        );
    }
    let number = crate::internal_plan::native_homebrew_checks::pull_request_number_from_env(event)?
        .ok_or_else(|| internal("homebrew_cutover_pull_request_missing"))?;
    let pull = crate::package_release_publish::read_public_pull_request(
        &transition.transition().tap_repository,
        transition.transition().tap_repository_id,
        number,
        deadline,
    )?;
    admit_cutover_author_pull(
        root,
        &pull,
        transition.transition(),
        &identity.tap_base_sha,
        &identity.head,
        &expected,
        deadline,
    )
}

pub(super) fn admit_cutover_author_pull(
    root: &Path,
    pull: &crate::package_release_publish::PublicPullRequestSnapshot,
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    expected_base: &str,
    checkout_head: &str,
    expected_author: &crate::github_landed_pull::GitHubRestUser,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let observed_author = pull
        .author
        .as_ref()
        .ok_or_else(|| internal("homebrew_cutover_pull_author"))?;
    if pull.state != "open"
        || pull.draft
        || pull.merged
        || pull.base_repository != transition.tap_repository
        || pull.base_repository_id != transition.tap_repository_id
        || pull.head_repository != transition.tap_repository
        || pull.head_repository_id != transition.tap_repository_id
        || pull.base_ref != "main"
        || pull.base_sha != expected_base
    {
        return Err(internal("homebrew_cutover_author_mismatch"));
    }
    crate::homebrew::structural_cutover::require_author(expected_author, observed_author)?;
    crate::homebrew::authority::verify_homebrew_authority_pull_request_checkout(
        root,
        pull,
        expected_base,
        &pull.head_sha,
        checkout_head,
        deadline,
    )
}

fn verify_cutover_author_merge_group_responses(
    identity: &crate::internal_plan::native_homebrew_checks::NativeCheckIdentityExtension,
    selection: &crate::package_release_publish::MergeGroupSelection,
    queue_response: &[u8],
    pull_response: &[u8],
    expected_author: &crate::github_landed_pull::GitHubRestUser,
) -> Result<(), OrchestratorError> {
    let transition = crate::homebrew::structural_cutover::protected_transition()?;
    let associated = crate::package_release_publish::parse_merge_group_pull_request_response(
        queue_response,
        selection,
    )?;
    let pull = crate::package_release_publish::parse_public_pull_request_response(
        pull_response,
        &transition.transition().tap_repository,
        transition.transition().tap_repository_id,
        associated.number,
    )?;
    admit_cutover_author_merge_group_pull(
        &pull,
        &associated,
        selection,
        transition.transition(),
        &identity.tap_base_sha,
        &identity.head,
        expected_author,
    )
}

fn admit_cutover_author_merge_group_pull(
    pull: &crate::package_release_publish::PublicPullRequestSnapshot,
    associated: &crate::package_release_publish::MergeGroupPullRequest,
    selection: &crate::package_release_publish::MergeGroupSelection,
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    expected_base: &str,
    checkout_head: &str,
    expected_author: &crate::github_landed_pull::GitHubRestUser,
) -> Result<(), OrchestratorError> {
    let observed_author = pull
        .author
        .as_ref()
        .ok_or_else(|| internal("homebrew_cutover_pull_author"))?;
    if selection.repository() != transition.tap_repository
        || selection.repository_id() != transition.tap_repository_id
        || selection.base_sha() != expected_base
        || selection.head_sha() != checkout_head
        || associated.base_repository != transition.tap_repository
        || associated.base_repository_id != transition.tap_repository_id
        || associated.head_repository != transition.tap_repository
        || associated.head_repository_id != transition.tap_repository_id
        || associated.base_sha != expected_base
        || pull.state != "open"
        || pull.draft
        || pull.merged
        || pull.base_repository != associated.base_repository
        || pull.base_repository_id != associated.base_repository_id
        || pull.base_ref != "main"
        || pull.base_ref != associated.base_ref
        || pull.base_sha != associated.base_sha
        || pull.head_repository != associated.head_repository
        || pull.head_repository_id != associated.head_repository_id
        || pull.head_ref != associated.head_ref
        || pull.head_sha != associated.head_sha
    {
        return Err(internal("homebrew_cutover_author_mismatch"));
    }
    crate::homebrew::structural_cutover::require_author(expected_author, observed_author)
}
