// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{RepoPath, RustMetadata};

use crate::error::ReleaseError;
use crate::registry::within;
use crate::release_plz::RootConfig;
use crate::tag::TagPattern;

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

fn ends_in_lock(version: &str) -> bool {
    version.as_bytes().ends_with(b".lock")
}

fn literal_version(text: &str) -> Option<(String, String)> {
    let document = text.parse::<toml::Table>().ok()?;
    let package = document.get("package")?;
    let name = package.get("name")?.as_str()?;
    let version = package.get("version")?.as_str()?;
    Some((name.to_owned(), version.to_owned()))
}

fn check_metadata(
    config: &RootConfig,
    metadata: &BTreeMap<RepoPath, RustMetadata>,
    manifests: &BTreeMap<String, String>,
) -> Result<(), ReleaseError> {
    for member in metadata.values().flat_map(|known| &known.members) {
        let path = member.manifest.as_str();
        if !within(&config.root, path) || !receives_version_tag(config, member.name.as_str()) {
            continue;
        }
        let version = match &member.version {
            Some(version) => Some(version.clone()),
            None => manifests
                .get(path)
                .and_then(|text| literal_version(text))
                .filter(|(name, _)| name == member.name.as_str())
                .map(|(_, version)| version),
        };
        if let Some(version) = version
            && ends_in_lock(&version)
        {
            return Err(ReleaseError::VersionEndsInLock(path.to_owned(), version));
        }
    }
    Ok(())
}

fn check_text(
    config: &RootConfig,
    manifests: &BTreeMap<String, String>,
) -> Result<(), ReleaseError> {
    for (path, text) in manifests
        .iter()
        .filter(|(path, _)| within(&config.root, path))
    {
        if let Some((name, version)) = literal_version(text)
            && receives_version_tag(config, &name)
            && ends_in_lock(&version)
        {
            return Err(ReleaseError::VersionEndsInLock(path.clone(), version));
        }
    }
    Ok(())
}

pub(crate) fn check_versions(
    configs: &[RootConfig],
    manifests: &BTreeMap<String, String>,
    metadata: Option<&BTreeMap<RepoPath, RustMetadata>>,
) -> Result<(), ReleaseError> {
    for config in configs.iter().filter(|config| {
        config
            .packages
            .iter()
            .map(|package| &package.tag_pattern)
            .chain(&config.workspace_tags)
            .any(TagPattern::ends_with_version)
    }) {
        match metadata {
            Some(metadata) => check_metadata(config, metadata, manifests)?,
            None => check_text(config, manifests)?,
        }
    }
    Ok(())
}
