// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{CutoverInput, history::ObservedCutoverPull};
use crate::homebrew::structural_cutover::CutoverMarker;
use crate::{OrchestratorError, internal::internal, tap_prepare::CommitResponse};
use velnor_actions_mise::CheckDeadline;

#[cfg(test)]
#[path = "homebrew_cutover_replay_tests.rs"]
mod tests;

pub(super) fn verify(
    input: &CutoverInput<'_>,
    marker: &CutoverMarker,
    pull: &ObservedCutoverPull,
    api: &super::super::github::TapMergeGitHub,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let transition = crate::homebrew::structural_cutover::protected_transition()?;
    let repository_id = transition.transition().tap_repository_id;
    let authority = crate::homebrew::authority::load_cutover_replay_authority(
        &input.policy.repository,
        repository_id,
        &pull.base,
        &pull.head,
        &pull.landed,
        &marker.source_sha,
        deadline,
    )?;
    if authority.plan() != input.plan
        || authority.tap() != input.policy
        || input.spec != Some(authority.spec())
        || marker.source_sha != input.release.receipt.source.commit_sha
        || marker.preview_receipt_digest != input.release.receipt_digest
        || marker.preview_formula_sha256
            != crate::homebrew::structural_cutover::sha256(input.formula)
        || marker.locator_blob_sha != authority.locator_blob_sha()
        || marker.seed.sdk_version != env!("CARGO_PKG_VERSION")
        || marker.seed.seed_output_base_sha != pull.base
    {
        return Err(internal("homebrew_cutover_initialization_binding"));
    }
    verify_sdk(&marker.seed, input.sdk_artifact_digest, deadline)?;
    let tree = crate::package_release_publish::read_protected_source_tree_identity(
        &input.policy.repository,
        repository_id,
        &pull.head,
        deadline,
    )?;
    if !tree.nonregular_entries.is_empty() || tree.regular_files.len() > 10_000 {
        return Err(internal("homebrew_cutover_initialization_tree"));
    }
    let landed: CommitResponse = api.get(&format!(
        "repos/{}/git/commits/{}",
        input.policy.repository, pull.landed
    ))?;
    validate_landed_commit(&landed, &pull.landed, &pull.base, &tree.tree_sha)?;
    let capture = super::checkout::capture(
        &input.policy.repository,
        &pull.base,
        &pull.head,
        &pull.landed,
        pull.number,
        deadline,
    )?;
    let witness = crate::homebrew::structural_cutover::verify_seed_output_base(
        capture.path(),
        &authority,
        deadline,
    )?
    .ok_or_else(|| internal("homebrew_cutover_initialization_seed_missing"))?;
    if witness.identity() != &marker.seed {
        return Err(internal("homebrew_cutover_initialization_seed_mismatch"));
    }
    crate::homebrew::structural_cutover::validate_structural_candidate(
        capture.path(),
        &authority,
        &witness,
        deadline,
    )
}

fn verify_sdk(
    seed: &crate::homebrew::structural_cutover::HomebrewSeedOutputBaseIdentity,
    host_digest: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let transition = crate::homebrew::structural_cutover::protected_transition()?;
    let binding = crate::homebrew::sdk_binding::verify_published_sdk_binding(
        &transition,
        &seed.sdk_version,
        &seed.sdk_source_sha,
        &seed.sdk_manifest_sha256,
        deadline,
    )?;
    if binding.identity().host_binary_sha256 != host_digest
        || binding.identity().seed_runner_binary_sha256 != seed.sdk_seed_runner_binary_sha256
    {
        return Err(internal("homebrew_cutover_initialization_sdk_mismatch"));
    }
    Ok(())
}

pub(super) fn validate_landed_commit(
    observed: &CommitResponse,
    expected_sha: &str,
    expected_base: &str,
    expected_tree: &str,
) -> Result<(), OrchestratorError> {
    if ![expected_sha, expected_base, expected_tree]
        .into_iter()
        .all(super::history::valid_sha)
        || observed.sha != expected_sha
        || observed.tree.sha != expected_tree
        || observed.parents.len() != 1
        || observed.parents[0].sha != expected_base
    {
        return Err(internal("homebrew_cutover_initialization_landed_commit"));
    }
    Ok(())
}
