// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use toml::Value;

use crate::error::ReleaseError;
use crate::registry::within;
use crate::release_plz::RootConfig;
use crate::tag::TagPattern;
use crate::workspace_root::{unparsable_package_name, workspace_root};

fn parse(manifests: &BTreeMap<String, String>, path: &str) -> Result<Value, ReleaseError> {
    manifests[path]
        .parse::<toml::Table>()
        .map(Value::Table)
        .map_err(|error| ReleaseError::InvalidToml {
            file: path.to_owned(),
            message: error.to_string(),
        })
}

fn workspace_version(
    manifests: &BTreeMap<String, String>,
    manifest: &str,
    package: &Value,
) -> Result<Option<String>, ReleaseError> {
    let Some(root) = workspace_root(manifests, manifest, package, parse)? else {
        return Ok(None);
    };
    Ok(parse(manifests, &root)?
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(Value::as_str)
        .map(str::to_owned))
}

fn effective_version(
    manifests: &BTreeMap<String, String>,
    path: &str,
    package: &Value,
) -> Result<Option<String>, ReleaseError> {
    match package.get("version") {
        Some(Value::String(version)) => Ok(Some(version.clone())),
        Some(Value::Table(table)) if table.get("workspace") == Some(&Value::Boolean(true)) => {
            workspace_version(manifests, path, package)
        }
        _ => Ok(None),
    }
}

fn receives_version_tag(config: &RootConfig, name: &str) -> bool {
    match config.packages.iter().find(|package| package.name == name) {
        Some(package) => package.tag_pattern.ends_with_version(),
        None => {
            config.releases(name)
                && config
                    .workspace_tags
                    .iter()
                    .any(TagPattern::ends_with_version)
        }
    }
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
        for path in manifests.keys().filter(|path| within(&config.root, path)) {
            let document = match parse(manifests, path) {
                Ok(document) => document,
                Err(error) => match unparsable_package_name(&manifests[path]) {
                    Some(name) if receives_version_tag(config, &name) => return Err(error),
                    _ => continue,
                },
            };
            let Some(package) = document.get("package") else {
                continue;
            };
            let Some(name) = package.get("name").and_then(Value::as_str) else {
                continue;
            };
            if !receives_version_tag(config, name) {
                continue;
            }
            if let Some(version) = effective_version(manifests, path, package)?
                .filter(|version| version.as_bytes().ends_with(b".lock"))
            {
                return Err(ReleaseError::VersionEndsInLock(path.clone(), version));
            }
        }
    }
    Ok(())
}
