//! Read-only source-main freshness check at downstream tap mutation boundaries.

use std::{
    ffi::{OsStr, OsString},
    time::Duration,
};

use serde_json::Value;
use velnor_actions_contract::{is_valid_branch_name, vcs::is_safe_github_repository};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

use crate::{OrchestratorError, internal::internal};

const SOURCE_API_TIMEOUT: Duration = Duration::from_secs(60);
const RESPONSE_LIMIT: usize = 1024 * 1024;

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
        || !valid_sha(expected_sha)
    {
        return Err(internal("tap_source_identity_invalid"));
    }
    let token = std::env::var("GH_TOKEN")
        .ok()
        .filter(|token| !token.is_empty())
        .ok_or_else(|| internal("tap_source_read_token_missing"))?;
    if std::env::var("VELNOR_TAP_TOKEN").is_ok_and(|value| value == token)
        || std::env::var("VELNOR_TAP_MERGE_TOKEN").is_ok_and(|value| value == token)
    {
        return Err(internal("tap_source_read_token_reused"));
    }
    let route = format!("repos/{repository}/commits/{}", encode_segment(branch));
    let args = vec![
        OsString::from("api"),
        OsString::from("-H"),
        OsString::from("Accept: application/vnd.github+json"),
        OsString::from("-H"),
        OsString::from("X-GitHub-Api-Version: 2026-03-10"),
        OsString::from(route),
    ];
    let command = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
        .map_err(|_| internal("tap_source_gh_unavailable"))?
        .command_with_github_token(&ToolCatalog::pinned(), &token)
        .map_err(|_| internal("tap_source_gh_unavailable"))?;
    let output = command
        .run_bounded(RESPONSE_LIMIT, SOURCE_API_TIMEOUT)
        .map_err(|_| internal("tap_source_head_lookup_failed"))?;
    let response: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| internal("tap_source_head_response_invalid"))?;
    if !output.success || response["sha"].as_str() != Some(expected_sha) {
        return Err(internal("tap_source_release_superseded"));
    }
    Ok(())
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn encode_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}
