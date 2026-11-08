// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{CrateReleaseIntent, CrateReleaseSourcePackage, OrchestratorError, Path};

pub(super) fn package_source(
    source: crate::CrateSource<'_>,
    intent: &CrateReleaseIntent,
    package: &CrateReleaseSourcePackage,
    target: &Path,
    sha: &str,
) -> Result<Vec<u8>, OrchestratorError> {
    source.execution.package(&crate::CratePackageSource::new(
        source.root,
        intent,
        package,
        target,
        sha,
        false,
        source.git,
    ))
}
