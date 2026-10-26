// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::{is_valid_branch_name, vcs::is_safe_github_repository};

pub(crate) fn verify_preview_source_is_current(
    repository: &str,
    branch: &str,
    repository_id: u64,
    expected_sha: &str,
) -> Result<bool, OrchestratorError> {
    if !is_safe_github_repository(repository)
        || !is_valid_branch_name(branch)
        || branch != "main"
        || !valid_sha(expected_sha)
    {
        return Err(internal("tap_source_identity_invalid"));
    }
    let token = std::env::var("GH_TOKEN")
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("tap_source_read_token_missing"))?;
    if [
        "VELNOR_TAP_TOKEN",
        "VELNOR_TAP_MERGE_TOKEN",
        "VELNOR_TAP_PROTECTION_TOKEN",
    ]
    .into_iter()
    .filter_map(|key| std::env::var(key).ok())
    .any(|writer| writer == token)
    {
        return Err(internal("tap_source_read_token_reused"));
    }
    let tip =
        crate::package_release_publish::verify_preview_main_tip(repository, repository_id, branch)?;
    if !is_current_preview_tip(expected_sha, &tip) {
        return Ok(false);
    }
    crate::package_release_publish::verify_current_source(repository, branch, expected_sha)?;
    Ok(true)
}

fn is_current_preview_tip(source_sha: &str, protected_tip: &str) -> bool {
    valid_sha(source_sha) && valid_sha(protected_tip) && source_sha == protected_tip
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
