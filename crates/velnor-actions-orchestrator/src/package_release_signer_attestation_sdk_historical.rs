// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CheckDeadline, OrchestratorError, SdkManifestAttestationExpectation, bound_release_manifest,
    verify_published_manifest_attestation,
};
use crate::internal::internal;

/// Verify old tool pins from a signed release manifest and the immutable tool catalogs.
/// The manifest and bundle must be acquired separately; no receipt field supplies an expected hash.
pub(crate) fn verify_historical_tool_expectations(
    manifest_path: &std::path::Path,
    bundle_path: &std::path::Path,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    receipt: &crate::crate_release_journal::JournalReceipt,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_release_historical_tools_invalid");
    if !matches!(intent.runner.as_str(), "ubuntu-24.04" | "ubuntu-26.04") {
        return Err(invalid());
    }
    let mut normalized = intent.clone();
    "ubuntu-26.04".clone_into(&mut normalized.runner);
    normalized.validate().map_err(|_| invalid())?;
    let verified = verify_published_manifest_attestation(
        manifest_path,
        bundle_path,
        &SdkManifestAttestationExpectation {
            repository: "tailrocks/velnor-new",
            repository_id: 1_390_620_900,
            owner_id: 78_806_509,
            source_sha: &intent.generator_source_sha,
            manifest_sha256: &intent.generator_manifest_sha256,
        },
        deadline,
    )?;
    let parent = manifest_path.parent().ok_or_else(invalid)?;
    let name = manifest_path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(invalid)?;
    let bytes = match crate::safe_read::read_repo_bytes(parent, name, 1024 * 1024)? {
        crate::safe_read::RepoBytes::Bytes(bytes) => bytes,
        crate::safe_read::RepoBytes::Absent => return Err(invalid()),
    };
    let manifest = bound_release_manifest(&bytes, &verified)?;
    let generator = manifest
        .record_for_target("x86_64-unknown-linux-gnu")
        .ok_or_else(invalid)?;
    let release_plz = velnor_actions_mise::catalog::qualified_release_plz(
        &intent.release_plz_version,
        &intent.runner,
    )
    .ok_or_else(invalid)?;
    let rust = velnor_actions_mise::catalog::rust_channel_manifest_sha256(&intent.rust_version)
        .ok_or_else(invalid)?;
    if manifest.repository != "tailrocks/velnor-new"
        || manifest.commit != intent.generator_source_sha
        || manifest.version != intent.generator_version
        || verified.workflow_sha != manifest.commit
        || verified.event != "workflow_dispatch"
        || receipt.generator_version != intent.generator_version
        || receipt.release_plz_version != intent.release_plz_version
        || receipt.rust_version != intent.rust_version
        || receipt.expected_tools.generator_sha256 != generator.sha256
        || receipt.expected_tools.release_plz_sha256 != release_plz.executable_sha256
        || receipt.expected_tools.rust_manifest_sha256 != rust
    {
        return Err(invalid());
    }
    Ok(())
}
