// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{
    RELEASE_MANIFEST_FILENAME, ReleaseManifest, ReleaseTarget, check_release_artifact,
};

use super::ReleaseHelperError;

pub(super) fn validated_consumer_manifest(
    target: ReleaseTarget,
    version: &str,
    json: Option<&str>,
) -> Result<ReleaseManifest, ReleaseHelperError> {
    let Some(json) = json else {
        return Err(ReleaseHelperError::Problem(
            "consumer_requires_release_install:install an official velnor-actions release"
                .to_owned(),
        ));
    };
    let manifest = ReleaseManifest::parse_json(json, RELEASE_MANIFEST_FILENAME)
        .map_err(ReleaseHelperError::Contract)?;
    manifest
        .validate(RELEASE_MANIFEST_FILENAME)
        .map_err(ReleaseHelperError::Contract)?;
    if manifest.version != version {
        return Err(ReleaseHelperError::Problem(format!(
            "release_manifest_version_mismatch:{}:{version}",
            manifest.version
        )));
    }
    super::release_binary::verify_current_exe_matches_consumer_manifest(&manifest)?;
    let record = manifest.record_for_target(target.triple()).ok_or_else(|| {
        ReleaseHelperError::Problem(format!("manifest_missing_target:{}", target.triple()))
    })?;
    check_release_artifact(
        &record.artifact,
        &manifest.version,
        &manifest.commit,
        target.triple(),
        RELEASE_MANIFEST_FILENAME,
        "targets.artifact",
    )
    .map_err(ReleaseHelperError::Contract)?;
    Ok(manifest)
}
