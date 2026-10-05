//! Read and select the committed consumer release manifest.

use std::path::Path;

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

/// Consumer manifest text plus stand-in flag: committed file, else a
/// debug-only stand-in.
///
/// Absent files fall back to the embedded stand-in (flagged so
/// `generate` warns loudly); present-but-unreadable files error
/// instead of masking. The stand-in carries the bound official-asset
/// URL shape so the consumer gate validates it exactly like a
/// committed file.
/// # Errors
///
/// Returns IO or unsafe-path errors for present-but-unreadable files.
#[cfg(debug_assertions)]
pub(crate) fn consumer_manifest_text(
    root: &Path,
) -> Result<(Option<String>, bool), OrchestratorError> {
    if let Some(text) = read_manifest_file(root)? {
        return Ok((Some(text), false));
    }
    let sha = "a".repeat(64);
    let version = env!("CARGO_PKG_VERSION");
    let mut targets = Vec::new();
    for target in velnor_actions_contract::ReleaseTarget::ALL {
        targets.push(format!(
            "{{\"target\":\"{}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{}\",\"sha256\":\"{sha}\"}}",
            target.triple(),
            target.triple()
        ));
    }
    let targets = targets.join(",");
    let commit = "b".repeat(40);
    Ok((
        Some(format!(
            "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{commit}\",\"targets\":[{targets}]}}"
        )),
        true,
    ))
}

/// Consumer manifest text plus stand-in flag: the committed file only.
///
/// Absent files stay `None` (the consumer acquire gate fails closed
/// with `consumer_requires_release_install`); unreadable files error.
/// The flag is always false: release builds have no stand-in.
/// # Errors
///
/// Returns IO or unsafe-path errors for present-but-unreadable files.
#[cfg(not(debug_assertions))]
pub(crate) fn consumer_manifest_text(
    root: &Path,
) -> Result<(Option<String>, bool), OrchestratorError> {
    read_manifest_file(root).map(|text| (text, false))
}
