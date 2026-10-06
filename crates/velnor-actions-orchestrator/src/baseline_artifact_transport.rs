//! ID-bound baseline ZIP retrieval and authentication.
//!
//! The service receipt stays separate from `BaselineManifest`: it proves the
//! fetched bytes, while the manifest's artifact ID fingerprints its name.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use velnor_actions_mise::{PinnedTool, PinnedToolExec, RuntimePaths, ToolCatalog};

use crate::merge::BaselineManifest;
use crate::run_select::BaselineArtifactReceipt;

#[path = "baseline_artifact_zip.rs"]
mod zip_archive;

/// Maximum compressed service archive size.
pub(crate) const MAX_BASELINE_ARCHIVE_BYTES: usize = 8 * 1024 * 1024;

/// Service bytes parsed and bound to their exact manifest identity.
pub(crate) struct VerifiedBaselineArchive {
    receipt: BaselineArtifactReceipt,
    manifest: BaselineManifest,
}

impl VerifiedBaselineArchive {
    /// Authenticated manifest identity, including its original run attempt.
    pub(crate) fn manifest(&self) -> &BaselineManifest {
        &self.manifest
    }
}

/// Exact GitHub API record for the successful source run attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BaselineAttemptReceipt {
    /// Numeric GitHub run ID.
    pub(crate) run_id: u64,
    /// Original workflow run attempt proven by the manifest and API response.
    pub(crate) run_attempt: u64,
}

/// Manifest and API receipts retained through authenticated lineage checks.
#[derive(Debug)]
pub(crate) struct AcquiredBaseline {
    manifest: BaselineManifest,
    attempt_receipt: BaselineAttemptReceipt,
}

impl AcquiredBaseline {
    /// Authenticated baseline manifest for coverage and lineage checks.
    pub(crate) fn manifest(&self) -> &BaselineManifest {
        &self.manifest
    }

    /// Exact API attempt receipt; binds run and attempt independently of name.
    pub(crate) fn attempt_receipt(&self) -> &BaselineAttemptReceipt {
        &self.attempt_receipt
    }
}

/// Download one artifact by its immutable service ID with a hard stdout cap.
///
/// The content digest is checked by [`verify_archive`] before ZIP parsing.
/// # Errors
/// Rejects nonzero exits, wrong byte count, and oversized process output.
pub(crate) fn download_archive(
    catalog: &ToolCatalog,
    root: &Path,
    lookup: &super::BaselineLookup,
    receipt: &BaselineArtifactReceipt,
    runtime: RuntimePaths,
) -> Result<Vec<u8>, String> {
    if receipt.service_id == 0
        || receipt.size_bytes == 0
        || receipt.size_bytes > MAX_BASELINE_ARCHIVE_BYTES as u64
    {
        return Err("baseline_artifact_size_invalid".to_owned());
    }
    let args = artifact_archive_args(lookup, receipt)?;
    let exec = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
        .map_err(|_| "baseline_unavailable".to_owned())?;
    let output = exec
        .command_with_runtime(catalog, runtime)
        .map_err(|_| "baseline_unavailable".to_owned())?
        .with_cwd(root.to_path_buf())
        .run()
        .map_err(|_| "baseline_unavailable".to_owned())?;
    if !output.success
        || output.stdout.len() > MAX_BASELINE_ARCHIVE_BYTES
        || u64::try_from(output.stdout.len()).ok() != Some(receipt.size_bytes)
    {
        return Err("baseline_archive_unavailable".to_owned());
    }
    Ok(output.stdout)
}

/// Verify service SHA-256 before parsing a bounded single-file ZIP.
///
/// The returned wrapper can only be built after archive, manifest, and exact
/// service/name/run binding all succeed. Attempt binding follows via API.
/// # Errors
/// Rejects digest mismatch, unsafe ZIPs, noncanonical JSON, or identity drift.
pub(crate) fn verify_archive(
    receipt: BaselineArtifactReceipt,
    bytes: &[u8],
) -> Result<VerifiedBaselineArchive, String> {
    if bytes.len() > MAX_BASELINE_ARCHIVE_BYTES
        || u64::try_from(bytes.len()).ok() != Some(receipt.size_bytes)
    {
        return Err("baseline_archive_unavailable".to_owned());
    }
    if sha256(bytes) != receipt.sha256 {
        return Err("baseline_artifact_digest_mismatch".to_owned());
    }
    let text = zip_archive::baseline_payload(bytes)?;
    let value = crate::internal_plan::snapshot::parse_canonical_json(&text)
        .map_err(|_| "baseline_unavailable".to_owned())?;
    let manifest: BaselineManifest =
        serde_json::from_value(value).map_err(|_| "baseline_unavailable".to_owned())?;
    if !manifest_matches_receipt(&manifest, &receipt) {
        return Err("baseline_artifact_manifest_mismatch".to_owned());
    }
    Ok(VerifiedBaselineArchive { receipt, manifest })
}

/// Authenticate the exact manifest attempt through GitHub's attempt API.
///
/// Artifact names carry only source and compatibility. The selected run and
/// service artifact receipt bind the run; the manifest identifies its attempt,
/// then the exact API attempt response proves that same attempt.
/// # Errors
/// Rejects any attempt record that does not prove the published manifest.
pub(crate) fn authenticate_attempt(
    verified: VerifiedBaselineArchive,
    record: &str,
    lookup: &super::BaselineLookup,
) -> Result<AcquiredBaseline, String> {
    let attempt = attempt_receipt(record, lookup, &verified.receipt, &verified.manifest)?;
    if verified.receipt.head_sha != lookup.base_sha
        || verified.receipt.head_branch != lookup.branch
        || !crate::retrieve_baseline::authentic_attempt(
            record,
            &verified.manifest,
            &lookup.repo,
            &lookup.branch,
            &lookup.workflow,
        )
    {
        return Err("baseline_unauthenticated".to_owned());
    }
    Ok(AcquiredBaseline {
        manifest: verified.manifest,
        attempt_receipt: attempt,
    })
}

/// Parse and bind the exact API receipt to the downloaded manifest.
/// # Errors
/// Refuses an attempt response for another run, attempt, source, or workflow.
fn attempt_receipt(
    text: &str,
    lookup: &super::BaselineLookup,
    receipt: &BaselineArtifactReceipt,
    manifest: &BaselineManifest,
) -> Result<BaselineAttemptReceipt, String> {
    let run = velnor_actions_contract::parse_strict_json(text)
        .map_err(|_| "baseline_unavailable".to_owned())?;
    let repository = run["repository"]["full_name"]
        .as_str()
        .and_then(crate::origin::validate_repository_slug);
    let head_repository = run["head_repository"]["full_name"]
        .as_str()
        .and_then(crate::origin::validate_repository_slug);
    let expected_repository = crate::origin::validate_repository_slug(&lookup.repo);
    if receipt.run_id == 0
        || manifest.run_attempt == 0
        || receipt.head_sha != lookup.base_sha
        || receipt.head_branch != lookup.branch
        || run["id"].as_u64() != Some(receipt.run_id)
        || run["run_attempt"].as_u64() != Some(manifest.run_attempt)
        || run["head_sha"] != receipt.head_sha
        || run["head_branch"] != receipt.head_branch
        || run["event"] != "push"
        || !attempt_path_matches(&run["path"], &lookup.workflow, &lookup.branch)
        || repository != expected_repository
        || head_repository != expected_repository
    {
        return Err("baseline_unauthenticated".to_owned());
    }
    if run["status"] != "completed" || run["conclusion"] != "success" {
        return Err("baseline_unauthenticated".to_owned());
    }
    Ok(BaselineAttemptReceipt {
        run_id: receipt.run_id,
        run_attempt: manifest.run_attempt,
    })
}

/// GitHub attempt responses append `@<branch>` to workflow paths.
pub(crate) fn attempt_path_matches(
    actual: &serde_json::Value,
    expected_path: &str,
    branch: &str,
) -> bool {
    let Some(actual) = actual.as_str() else {
        return false;
    };
    actual == expected_path
        || actual
            .strip_suffix(&format!("@{branch}"))
            .is_some_and(|path| path == expected_path)
}

/// Keep the manifest bound to the service receipt and canonical two-part name.
fn manifest_matches_receipt(
    manifest: &BaselineManifest,
    receipt: &BaselineArtifactReceipt,
) -> bool {
    let canonical_name = velnor_actions_contract::artifact_id_for_baseline(
        &manifest.source_commit,
        &manifest.compatibility_id,
    );
    manifest.source_commit == receipt.head_sha
        && manifest.run_id == receipt.run_id
        && manifest.run_attempt > 0
        && manifest.compatibility_id == receipt.compatibility_id
        && manifest.artifact_name == receipt.name
        && canonical_name
            .as_ref()
            .is_ok_and(|name| name.as_str() == receipt.name.as_str())
        && manifest.artifact_id == crate::cover_compat::baseline_artifact_numeric_id(&receipt.name)
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn artifact_archive_args(
    lookup: &super::BaselineLookup,
    receipt: &BaselineArtifactReceipt,
) -> Result<Vec<OsString>, String> {
    if receipt.service_id == 0 {
        return Err("baseline_artifact_id_missing".to_owned());
    }
    let endpoint = format!(
        "repos/{}/actions/artifacts/{}/zip",
        lookup.repo, receipt.service_id
    );
    Ok(vec![OsString::from("api"), OsString::from(endpoint)])
}

#[cfg(test)]
#[path = "baseline_artifact_transport_tests.rs"]
mod tests;
