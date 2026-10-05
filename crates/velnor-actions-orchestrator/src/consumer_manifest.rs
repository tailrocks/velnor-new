//! Read and select the committed consumer release manifest.

use std::path::Path;

use velnor_actions_contract::WorkflowPolicy;

use crate::OrchestratorError;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

/// Committed consumer-manifest filename under `.velnor`.
const RELEASE_MANIFEST_REL: &str = ".velnor/release-manifest.json";

/// Read the committed release-manifest file; absent is `None`.
///
/// Cfg-independent so tests (debug assertions on) cover the exact read
/// the release twin relies on; schema and version validation happen
/// downstream in the consumer acquire gate. Present-but-unreadable
/// files (symlink, escape, oversize, bad UTF-8) error: an unreadable
/// manifest is never silently masked as absent (X6).
/// # Errors
///
/// Returns IO or unsafe-path errors for present-but-unreadable files.
pub(crate) fn read_manifest_file(root: &Path) -> Result<Option<String>, OrchestratorError> {
    match read_repo_file(root, RELEASE_MANIFEST_REL, MAX_REPO_FILE_BYTES)? {
        RepoRead::Absent => Ok(None),
        RepoRead::Text(text) => Ok(Some(text)),
    }
}

/// Admit the consumer manifest only under consumer policy.
///
/// Velnor repository policy gets bootstrap provenance from its generator lock.
/// A consumer manifest is not an input under that policy.
/// # Errors
///
/// Returns IO or unsafe-path errors for unreadable consumer manifests.
pub(crate) fn for_policy(
    root: &Path,
    policy: WorkflowPolicy,
) -> Result<Option<String>, OrchestratorError> {
    match policy {
        WorkflowPolicy::ConsumerV1 => read_manifest_file(root),
        WorkflowPolicy::VelnorRepositoryV1 => Ok(None),
    }
}
