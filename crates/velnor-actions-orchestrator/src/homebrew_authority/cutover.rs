// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{CheckDeadline, OrchestratorError, VerifiedHomebrewAuthority, internal};

pub(crate) fn load_cutover_replay_authority(
    repository: &str,
    repository_id: u64,
    base: &str,
    head: &str,
    landed: &str,
    source: &str,
    deadline: CheckDeadline,
) -> Result<VerifiedHomebrewAuthority, OrchestratorError> {
    let transition = crate::homebrew::structural_cutover::protected_transition()?;
    if repository != transition.transition().tap_repository
        || repository_id != transition.transition().tap_repository_id
        || ![base, head, landed, source]
            .into_iter()
            .all(super::lowercase_sha)
    {
        return Err(internal("homebrew_cutover_replay_selection"));
    }
    crate::package_release_publish::verify_public_protected_main_lineage(
        repository,
        repository_id,
        "main",
        base,
        landed,
        deadline,
    )?;
    let locator_bytes = super::protected_locator(repository, repository_id, base, deadline)?
        .ok_or_else(|| internal("homebrew_cutover_replay_locator_missing"))?;
    let locator = super::parse_homebrew_source_authority_locator(&locator_bytes)
        .map_err(|_| internal("homebrew_cutover_replay_locator_invalid"))?;
    let projection = super::load_source_projection(&locator, repository, deadline)?;
    if locator.default_branch != "main" || projection.source_tip != source {
        return Err(internal("homebrew_cutover_replay_source_changed"));
    }
    Ok(VerifiedHomebrewAuthority {
        source_config: projection.source_config,
        spec: projection.spec,
        tap: projection.tap,
        plan: projection.plan,
        source_repository_id: locator.repository_id,
        authority_sha: projection.source_tip,
        tap_repository: repository.to_owned(),
        tap_repository_id: repository_id,
        tap_base_sha: base.to_owned(),
        tap_head_sha: head.to_owned(),
        locator_blob_sha: crate::release_policy_admission::git_blob_sha1(&locator_bytes)?,
    })
}
