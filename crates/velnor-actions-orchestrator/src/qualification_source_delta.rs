//! Authoritative source comparison for the UsefulDelta experiment.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};
use velnor_actions_contract::{canonical_json_bytes, digest_b3, normalize_posix_path};
use velnor_actions_mise::GitRequest;

use crate::OrchestratorError;
use crate::internal::internal;
use crate::qualification_github_api::QualificationGitHubApi;

/// Maximum NUL-delimited path-list bytes captured from Git.
const MAX_SOURCE_DIFF_BYTES: usize = 512 * 1024;
/// Maximum changed paths retained in one source-delta receipt.
const MAX_SOURCE_DIFF_PATHS: usize = 256;
/// One bounded git read operation.
const GIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Compare the admitted predecessor source with the checked-out source.
///
/// GitHub's compare endpoint proves ancestry; local Git supplies the exact
/// sorted path set whose canonical digest is stored in the admission.
///
/// # Errors
/// Returns an internal error for a missing source object, non-ancestor,
/// empty or oversized diff, non-UTF-8 path, or invalid repository path.
pub(crate) fn source_delta(
    api: &QualificationGitHubApi,
    root: &Path,
    base_sha: &str,
    source_sha: &str,
) -> Result<Value, OrchestratorError> {
    require_checkout_sha(root, source_sha)?;
    if !api.base_is_ancestor(base_sha, source_sha)? {
        return Err(internal("qualification_source_base_not_ancestor"));
    }
    let paths = changed_paths(root, base_sha, source_sha)?;
    if paths.is_empty() || paths.len() > MAX_SOURCE_DIFF_PATHS {
        return Err(internal("qualification_source_diff_size_invalid"));
    }
    validate_paths(&paths)?;
    let ordered: Vec<String> = paths.into_iter().collect();
    let digest =
        digest_b3(&canonical_json_bytes(&ordered).map_err(crate::internal::internal_contract)?);
    Ok(json!({
        "base_source_sha": base_sha,
        "source_sha": source_sha,
        "changed_paths": ordered,
        "diff_digest": digest,
        "base_is_ancestor": true
    }))
}

fn require_checkout_sha(root: &Path, expected: &str) -> Result<(), OrchestratorError> {
    if !is_lower_sha(expected) {
        return Err(internal("qualification_source_sha_invalid"));
    }
    let output = GitRequest::rev_parse(vec![OsString::from("HEAD")])
        .command_in(root)
        .run_bounded(1024, GIT_TIMEOUT)
        .map_err(|_| internal("qualification_source_checkout_unavailable"))?;
    if !output.success {
        return Err(internal("qualification_source_checkout_unavailable"));
    }
    let actual = output
        .stdout_text("git")
        .map_err(|_| internal("qualification_source_checkout_invalid"))?
        .trim()
        .to_owned();
    if actual != expected {
        return Err(internal("qualification_source_checkout_mismatch"));
    }
    Ok(())
}

fn changed_paths(
    root: &Path,
    base: &str,
    source: &str,
) -> Result<BTreeSet<String>, OrchestratorError> {
    if !is_lower_sha(base) || !is_lower_sha(source) || base == source {
        return Err(internal("qualification_source_range_invalid"));
    }
    let mut args = vec![
        OsString::from("--name-only"),
        OsString::from("--no-renames"),
        OsString::from(format!("{base}...{source}")),
        OsString::from("--"),
    ];
    crate::validators::validate_select_diff_args(&args)
        .map_err(|_| internal("qualification_source_range_invalid"))?;
    args.insert(0, OsString::from("-z"));
    let output = GitRequest::diff(args)
        .command_in(root)
        .run_bounded(MAX_SOURCE_DIFF_BYTES, GIT_TIMEOUT)
        .map_err(|_| internal("qualification_source_diff_unavailable"))?;
    if !output.success {
        return Err(internal("qualification_source_diff_unavailable"));
    }
    crate::git_paths::split_nul_paths(&output.stdout)
        .map_err(|_| internal("qualification_source_diff_path_invalid"))
}

fn validate_paths(paths: &BTreeSet<String>) -> Result<(), OrchestratorError> {
    for path in paths {
        let normalized = normalize_posix_path(path).map_err(crate::internal::internal_contract)?;
        if normalized != *path {
            return Err(internal("qualification_source_diff_path_invalid"));
        }
    }
    Ok(())
}

fn is_lower_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
