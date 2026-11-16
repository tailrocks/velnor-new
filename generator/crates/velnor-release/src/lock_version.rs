// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use toml::Value;

use crate::error::ReleaseError;
use crate::registry::within;
use crate::release_plz::RootConfig;
use crate::tag::TagPattern;

fn versions(text: &str) -> Vec<String> {
    let Ok(document) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let own = document
        .get("package")
        .and_then(|package| package.get("version"));
    let shared = document
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"));
    [own, shared]
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

pub(crate) fn check_versions(
    configs: &[RootConfig],
    manifests: &BTreeMap<String, String>,
) -> Result<(), ReleaseError> {
    for config in configs.iter().filter(|config| {
        config
            .packages
            .iter()
            .map(|package| &package.tag_pattern)
            .chain(&config.workspace_tags)
            .any(TagPattern::ends_with_version)
    }) {
        for (path, text) in manifests
            .iter()
            .filter(|(path, _)| within(&config.root, path))
        {
            if let Some(version) = versions(text)
                .into_iter()
                .find(|version| version.as_bytes().ends_with(b".lock"))
            {
                return Err(ReleaseError::VersionEndsInLock(path.clone(), version));
            }
        }
    }
    Ok(())
}
