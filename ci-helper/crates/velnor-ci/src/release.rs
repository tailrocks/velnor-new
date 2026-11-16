// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Deserialize;

use velnor_model::{RepoPath, RustMetadata};
use velnor_release::is_tag_name;

use crate::error::CiError;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PlainName(String);

impl PlainName {
    pub fn new(what: &str, text: &str) -> Result<Self, CiError> {
        let plain = !text.is_empty()
            && !text.starts_with(['.', '-'])
            && !text.ends_with('.')
            && text.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'.' | b'_' | b'-')
                    || (what == "tag" && byte == b'+')
            });
        if plain {
            Ok(Self(text.to_owned()))
        } else {
            Err(CiError::new(format!(
                "{what} `{}` must be ASCII letters, digits, '.', '_' and '-', and must not start with '.' or '-' or end with '.'",
                text.escape_debug()
            )))
        }
    }

    pub fn tag(text: &str) -> Result<Self, CiError> {
        if is_tag_name(text) {
            Ok(Self(text.to_owned()))
        } else {
            Err(CiError::new(format!(
                "tag `{}` must be ASCII letters, digits, '.', '_', '-' and '+', and must not start with '.' or '-' or end with '.'",
                text.escape_debug()
            )))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PlainName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Deserialize)]
struct ReleasePlzOutput {
    releases: Vec<PackageRelease>,
}

#[derive(Debug, Deserialize)]
struct PackageRelease {
    package_name: String,
    tag: String,
}

pub fn record(outputs: &[(String, String)]) -> Result<String, CiError> {
    let mut tags: BTreeMap<PlainName, PlainName> = BTreeMap::new();
    let mut packages: BTreeMap<PlainName, PlainName> = BTreeMap::new();
    for (file, text) in outputs {
        let output: ReleasePlzOutput = serde_json::from_str(text).map_err(|e| {
            CiError::new(format!(
                "{file} is not the output of release-plz release -o json: {e}"
            ))
        })?;
        for release in output.releases {
            let package = output_name(&release.package_name)?;
            let tag = PlainName::tag(&release.tag)?;
            if let Some(other) = tags.insert(package.clone(), tag.clone()) {
                return Err(CiError::new(format!(
                    "release-plz released package {package} twice, as {other} and as {tag}"
                )));
            }
            if let Some(other) = packages.insert(tag.clone(), package.clone()) {
                return Err(CiError::new(format!(
                    "release-plz released {other} and {package} with the same tag {tag}"
                )));
            }
        }
    }
    Ok(tags
        .iter()
        .map(|(package, tag)| [package.as_str(), "=", tag.as_str(), "\n"].concat())
        .collect())
}

fn output_name(package: &str) -> Result<PlainName, CiError> {
    let name = PlainName::new("package", package)?;
    if package.starts_with(|first: char| first.is_ascii_alphabetic() || first == '_')
        && !package.contains('.')
    {
        Ok(name)
    } else {
        Err(CiError::new(format!(
            "package `{package}` cannot name a job output, which starts with a letter or '_' and holds no '.'"
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoPathEntry {
    pub path: String,
    pub symlink: bool,
    pub text: Option<String>,
}

pub fn check_cargo_discovery(
    entries: &[CargoPathEntry],
    publishes_to_crates_io: bool,
) -> Result<(), CiError> {
    let exists = |path: &str| {
        entries
            .iter()
            .any(|entry| entry.path == path && entry.text.is_some())
    };
    for entry in entries {
        if entry.symlink {
            return Err(CiError::new(format!(
                "{}: a symbolic link on the cargo config path can make cargo read a file the generator never saw",
                entry.path
            )));
        }
        if let Some(text) = &entry.text {
            velnor_release::refuse_cargo_config(
                &entry.path,
                text,
                publishes_to_crates_io,
                velnor_release::shadowed_by_extensionless_config(&entry.path, exists),
            )
            .map_err(CiError::new)?;
        }
    }
    Ok(())
}

pub fn publishes_to_crates_io(
    root: &str,
    text: &str,
    metadata: Option<&BTreeMap<RepoPath, RustMetadata>>,
) -> Result<bool, CiError> {
    velnor_release::root_publishes_to_crates_io(root, text, metadata).map_err(CiError::new)
}

pub fn check_config(files: &[(String, String)]) -> Result<String, CiError> {
    for (file, text) in files {
        velnor_release::refuse_external_commands(file, text).map_err(CiError::new)?;
    }
    velnor_release::check_release_plz_configs(files).map_err(CiError::new)?;
    Ok(String::new())
}

pub fn check_tag_patterns(
    files: &[(String, String)],
    expected: &[(String, String)],
) -> Result<(), CiError> {
    if expected.is_empty() {
        return Ok(());
    }
    for (file, text) in files {
        let want: BTreeSet<&str> = expected
            .iter()
            .filter(|(name, _)| name == file)
            .map(|(_, pattern)| pattern.as_str())
            .collect();
        let found = velnor_release::release_tag_patterns(file, text)
            .map_err(|e| CiError::new(e.to_string()))?;
        let found: BTreeSet<&str> = found.iter().map(String::as_str).collect();
        if found != want {
            return Err(CiError::new(format!(
                "{file}: git_tag_name patterns {found:?} differ from {want:?}, the ones the tag rulesets were provisioned for"
            )));
        }
    }
    Ok(())
}

pub fn check_manifests(
    files: &[(String, String)],
    metadata: &BTreeMap<RepoPath, RustMetadata>,
    manifests: &BTreeMap<String, String>,
    versions: &velnor_release::Versions,
) -> Result<(), CiError> {
    velnor_release::check_release_manifests(files, metadata, manifests).map_err(CiError::new)?;
    velnor_release::check_release_versions(files, metadata, versions).map_err(CiError::new)
}
