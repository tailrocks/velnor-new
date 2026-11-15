// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{RepoPath, RustPackage, TargetKind, TargetName};

use crate::toml_file::table;

pub(crate) fn custom_harnesses(
    files: &BTreeMap<RepoPath, Vec<u8>>,
    package: &RustPackage,
) -> Vec<TargetName> {
    let Some(manifest) = files.get(&package.manifest).and_then(|b| table(b).ok()) else {
        return Vec::new();
    };
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
    found
}
