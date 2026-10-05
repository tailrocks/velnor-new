//! Consumer release-manifest input and debug-only fixture data.

use std::path::Path;

use velnor_actions_contract::WorkflowPolicy;

use crate::OrchestratorError;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

const RELEASE_MANIFEST_REL: &str = ".velnor/release-manifest.json";

/// Read consumer manifest data only for the policy that uses it.
/// # Errors
/// Returns errors for present-but-unreadable manifest files.
pub(crate) fn for_policy(
    root: &Path,
    policy: WorkflowPolicy,
) -> Result<(Option<String>, bool), OrchestratorError> {
    if policy == WorkflowPolicy::ConsumerV1 {
        consumer_manifest_text(root)
    } else {
        Ok((None, false))
    }
}

/// Read the committed manifest; absent files stay `None`.
/// # Errors
/// Returns IO or unsafe-path errors for present-but-unreadable files.
pub(crate) fn read_manifest_file(root: &Path) -> Result<Option<String>, OrchestratorError> {
    match read_repo_file(root, RELEASE_MANIFEST_REL, MAX_REPO_FILE_BYTES)? {
        RepoRead::Absent => Ok(None),
        RepoRead::Text(text) => Ok(Some(text)),
    }
}

/// Debug consumers may use a clearly flagged manifest stand-in.
/// # Errors
/// Returns errors for present-but-unreadable manifest files.
#[cfg(debug_assertions)]
fn consumer_manifest_text(root: &Path) -> Result<(Option<String>, bool), OrchestratorError> {
    if let Some(text) = read_manifest_file(root)? {
        return Ok((Some(text), false));
    }
    let sha = "a".repeat(64);
    let version = env!("CARGO_PKG_VERSION");
    let targets = velnor_actions_contract::SUPPORTED_TARGETS
        .map(|target| format!(
            "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{sha}\"}}"
        ))
        .join(",");
    let commit = "b".repeat(40);
    Ok((
        Some(format!(
            "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{commit}\",\"targets\":[{targets}]}}"
        )),
        true,
    ))
}

/// Release consumers use only committed manifest data.
/// # Errors
/// Returns errors for present-but-unreadable manifest files.
#[cfg(not(debug_assertions))]
fn consumer_manifest_text(root: &Path) -> Result<(Option<String>, bool), OrchestratorError> {
    read_manifest_file(root).map(|text| (text, false))
}
