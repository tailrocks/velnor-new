//! Read-only source-main freshness check at downstream tap mutation boundaries.
//! Latest exact-source CI is independently reauthenticated before every mutation.

use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::{is_valid_branch_name, vcs::is_safe_github_repository};

/// Determine whether the signed preview is still the authenticated protected main tip.
/// # Errors
/// Rejects invalid identities, reused credentials, and API/protection failures.
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

#[cfg(test)]
#[test]
fn historical_preview_stays_valid_but_is_not_current_for_tap_promotion() {
    let source = "a".repeat(40);
    let authority = "b".repeat(40);
    let tip = "c".repeat(40);
    assert!(valid_sha(&source));
    assert!(valid_sha(&authority));
    assert!(!is_current_preview_tip(&source, &tip));
    assert!(is_current_preview_tip(&source, &source));
    assert!(!is_current_preview_tip(&"A".repeat(40), &tip));
}
