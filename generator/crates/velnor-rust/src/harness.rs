// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoView, RustPackage, TargetKind, TargetName};

use crate::error::{DetectError, Reason};
use crate::toml_file::table;

pub(crate) fn custom_harnesses(
    view: &RepoView,
    package: &RustPackage,
) -> Result<Vec<TargetName>, DetectError> {
    if !view.tracked_paths.contains(&package.manifest) {
        return Err(DetectError::invalid(
            &package.manifest,
            Reason::UntrackedManifest,
        ));
    }
    let Some(bytes) = view.files.get(&package.manifest) else {
        return Ok(Vec::new());
    };
    let manifest =
        table(bytes).map_err(|reason| DetectError::invalid(&package.manifest, reason))?;
    let declared = manifest
        .get("test")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("harness").and_then(toml::Value::as_bool) == Some(false))
        .filter_map(|entry| entry.get("name")?.as_str());
    let mut found: Vec<TargetName> = declared
        .filter_map(|name| {
            package
                .targets
                .iter()
                .find(|target| {
                    target.name.as_str() == name
                        && target.tests
                        && !target.requires_features
                        && target.kinds == [TargetKind::Test]
                })
                .map(|target| target.name.clone())
        })
        .collect();
    found.sort();
    found.dedup();
    Ok(found)
}
