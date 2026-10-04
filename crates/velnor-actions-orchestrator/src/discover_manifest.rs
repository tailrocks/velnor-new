//! Consumer release-manifest admission during discovery.

use std::path::Path;

use velnor_actions_contract::WorkflowPolicy;

use crate::OrchestratorError;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

/// Consumer-manifest filename under `.velnor`.
const RELEASE_MANIFEST_REL: &str = ".velnor/release-manifest.json";

/// Admit the consumer manifest only under consumer policy.
///
/// Velnor repository policy gets bootstrap provenance from
/// `.velnor/generator.lock` in `validate`; a consumer manifest is not an
/// input there. This policy check happens before any filesystem access.
///
/// # Errors
///
/// Returns IO or unsafe-path errors for unreadable consumer manifests.
pub(crate) fn for_policy(
    root: &Path,
    policy: WorkflowPolicy,
) -> Result<(Option<String>, bool), OrchestratorError> {
    match policy {
        WorkflowPolicy::ConsumerV1 => consumer_manifest_text(root),
        WorkflowPolicy::VelnorRepositoryV1 => Ok((None, false)),
    }
}

/// Read the committed consumer manifest; absent is `None`.
///
/// Schema and version validation happen downstream in the consumer acquire
/// gate. Present-but-unreadable files error; unreadable manifests never get
/// silently masked as absent (X6).
///
/// # Errors
///
/// Returns IO or unsafe-path errors for present-but-unreadable files.
pub(crate) fn read_manifest_file(root: &Path) -> Result<Option<String>, OrchestratorError> {
    match read_repo_file(root, RELEASE_MANIFEST_REL, MAX_REPO_FILE_BYTES)? {
        RepoRead::Absent => Ok(None),
        RepoRead::Text(text) => Ok(Some(text)),
    }
}

/// Consumer manifest text plus stand-in flag: committed file, else debug stand-in.
#[cfg(debug_assertions)]
fn consumer_manifest_text(root: &Path) -> Result<(Option<String>, bool), OrchestratorError> {
    if let Some(text) = read_manifest_file(root)? {
        return Ok((Some(text), false));
    }
    let sha = "a".repeat(64);
    let version = env!("CARGO_PKG_VERSION");
    let targets = [
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ]
    .map(|target| {
        format!(
            "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{sha}\"}}"
        )
    })
    .join(",");
    let commit = "b".repeat(40);
    Ok((
        Some(format!(
            "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{commit}\",\"targets\":[{targets}]}}"
        )),
        true,
    ))
}

/// Consumer manifest text plus stand-in flag: the committed file only.
#[cfg(not(debug_assertions))]
fn consumer_manifest_text(root: &Path) -> Result<(Option<String>, bool), OrchestratorError> {
    read_manifest_file(root).map(|text| (text, false))
}
