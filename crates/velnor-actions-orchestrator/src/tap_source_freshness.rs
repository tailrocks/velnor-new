//! Read-only source-main freshness check at downstream tap mutation boundaries.
//! Latest exact-source CI is independently reauthenticated before every mutation.

use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::{is_valid_branch_name, vcs::is_safe_github_repository};

/// Require the signed release commit to remain the source default-branch head.
/// # Errors
/// Rejects invalid identities, reused credentials, API failures, or superseded commits.
pub(crate) fn verify_preview_source_is_current(
    repository: &str,
    branch: &str,
    expected_sha: &str,
) -> Result<(), OrchestratorError> {
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
    crate::package_release_publish::verify_current_source(repository, branch, expected_sha)
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
