// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    OrchestratorError, crate_release_git::ReleaseGit, internal::internal,
    release::crate_release_source::CrateReleaseSourcePackage,
};
use std::path::Path;
use velnor_actions_contract::config::CrateReleaseIntent;

pub fn verify_reconstructed_archive(
    source: &Path,
    source_sha: &str,
    intent: &CrateReleaseIntent,
    package: &CrateReleaseSourcePackage,
    observed: &[u8],
    git: &ReleaseGit,
) -> Result<String, OrchestratorError> {
    intent.validate().map_err(|_| invalid())?;
    if !intent.packages.contains(&package.name)
        || !intent.pr_write_paths.contains(&package.manifest)
        || !velnor_actions_contract::config::is_cargo_version(&package.version)
        || intent.recovery.as_ref().is_some_and(|recovery| {
            recovery.source_sha != source_sha
                || recovery.package != package.name
                || recovery.version != package.version
        })
    {
        return Err(invalid());
    }
    crate::verify_crate_release_source(source, source_sha, git)?;
    let target = tempfile::tempdir().map_err(|_| invalid())?;
    let expected = crate::CrateSourceExecution::package(
        &crate::UnprivilegedSourceExecution,
        &crate::CratePackageSource::new(
            source,
            intent,
            package,
            target.path(),
            source_sha,
            false,
            git,
        ),
    )?;
    crate::release::crate_release_archive::verify_reconstructed_archive_tracked_content(
        &expected,
        source,
        source_sha,
        &package.manifest,
        &package.name,
        &package.version,
        git,
    )?;
    let checksum = crate::planner::generator::sha256_hex(observed);
    crate::verify_crate_archive(
        &expected,
        observed,
        &package.name,
        &package.version,
        &checksum,
    )?;

    crate::release::crate_release_archive::verify_reconstructed_archive_tracked_content(
        observed,
        source,
        source_sha,
        &package.manifest,
        &package.name,
        &package.version,
        git,
    )?;
    crate::verify_crate_release_source(source, source_sha, git)?;
    Ok(checksum)
}

fn invalid() -> OrchestratorError {
    internal("qualification_archive_reconstruction_invalid")
}

#[cfg(test)]
#[path = "../tests/crate_release_qualification_package.rs"]
mod tests;
