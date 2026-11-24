// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::ReleaseError;
use crate::plan::ReleaseFiles;
use crate::velnor_config::Binary;

fn defines(text: &str, profile: &str) -> bool {
    text.parse::<toml::Table>()
        .ok()
        .and_then(|table| table.get("profile")?.as_table()?.get(profile).cloned())
        .is_some()
}

pub(crate) fn check_profile(
    binary: &Binary,
    root_manifest: &str,
    files: &ReleaseFiles,
) -> Result<(), ReleaseError> {
    let Some(profile) = binary.profile.as_deref().filter(|name| *name != "release") else {
        return Ok(());
    };
    let Some(manifest) = files
        .cargo_manifest_texts
        .get(root_manifest)
        .filter(|text| !text.is_empty())
    else {
        return Ok(());
    };
    if defines(manifest, profile)
        || files
            .cargo_configs
            .values()
            .any(|config| defines(config, profile))
    {
        return Ok(());
    }
    Err(ReleaseError::UndefinedProfile(
        binary.package.clone(),
        profile.to_owned(),
        root_manifest.to_owned(),
    ))
}
