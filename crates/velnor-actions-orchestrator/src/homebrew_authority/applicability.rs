// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{OrchestratorError, internal};

use crate::package_release_publish::PublicRepositorySnapshot;

pub(super) fn confirm_locator_absence(
    expected_repository: &str,
    before: &PublicRepositorySnapshot,
    before_tip: &str,
    locator: Option<&[u8]>,
    after: &PublicRepositorySnapshot,
    after_tip: &str,
) -> Result<bool, OrchestratorError> {
    if !snapshot_matches(expected_repository, before)
        || !snapshot_matches(expected_repository, after)
    {
        return Err(internal("homebrew_authority_repository_identity"));
    }
    if before != after || before_tip != after_tip {
        return Err(internal("homebrew_authority_applicability_changed"));
    }
    Ok(locator.is_none())
}

fn snapshot_matches(repository: &str, snapshot: &PublicRepositorySnapshot) -> bool {
    snapshot.id > 0
        && snapshot.full_name == repository
        && velnor_actions_contract::vcs::is_safe_github_repository(repository)
        && valid_branch(&snapshot.default_branch)
}

fn valid_branch(branch: &str) -> bool {
    velnor_actions_contract::vcs::VcsInputs {
        commit: None,
        reference: Some(format!("refs/heads/{branch}")),
        submodules: std::collections::BTreeMap::new(),
    }
    .validate()
    .is_ok()
}
