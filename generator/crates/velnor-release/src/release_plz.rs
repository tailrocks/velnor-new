// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::ReleaseError;
use crate::hazards::external_command;
use crate::settings::{Document, Settings};
use crate::tag::{PACKAGE_WILDCARD, TagPattern, checked};

const DEFAULT_PR_BRANCH_PREFIX: &str = "release-plz-";
const DEFAULT_WORKSPACE_TAGS: [&str; 2] = ["{{ package }}-v{{ version }}", "v{{ version }}"];

#[derive(Debug, Clone)]
pub(crate) struct Package {
    pub(crate) name: String,
    pub(crate) tag_pattern: TagPattern,
}

#[derive(Debug, Clone)]
pub(crate) struct RootConfig {
    pub(crate) root: String,
    pub(crate) pr_branch_prefix: String,
    pub(crate) publishes_to_crates_io: bool,
    pub(crate) packages: Vec<Package>,
    pub(crate) workspace_tags: Vec<TagPattern>,
}

fn check_package(
    root: &str,
    package: Option<&str>,
    settings: &Settings,
) -> Result<(), ReleaseError> {
    let owned = || (root.to_owned(), package.map(str::to_owned));
    if settings.git_tag_enable == Some(false) {
        let (root, package) = owned();
        return Err(ReleaseError::GitTagDisabled { root, package });
    }
    if settings.git_release_enable == Some(false) {
        let (root, package) = owned();
        return Err(ReleaseError::GitReleaseDisabled { root, package });
    }
    if settings.release_always != Some(false) {
        return Err(ReleaseError::ReleaseAlwaysNotFalse {
            root: root.to_owned(),
        });
    }
    if settings.git_release_draft != Some(true) {
        let (root, package) = owned();
        return Err(ReleaseError::GitReleaseDraftNotTrue { root, package });
    }
    if settings.semver_check != Some(false) {
        let (root, package) = owned();
        return Err(ReleaseError::SemverCheckNotFalse { root, package });
    }
    if settings.publishes_to_crates_io() && settings.publish_no_verify != Some(true) {
        let (root, package) = owned();
        return Err(ReleaseError::PublishWithoutNoVerify { root, package });
    }
    Ok(())
}

pub(crate) fn valid_package_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn publishes(workspace: &Settings, released: &[(String, Settings)]) -> bool {
    (workspace.release != Some(false) && workspace.publishes_to_crates_io())
        || released
            .iter()
            .any(|(_, settings)| settings.publishes_to_crates_io())
}

fn parse_document(root: &str, text: &str) -> Result<Document, ReleaseError> {
    let invalid = |message: String| ReleaseError::InvalidToml {
        file: format!("{root}/release-plz.toml"),
        message,
    };
    let raw = text
        .parse::<toml::Table>()
        .map_err(|error| invalid(error.to_string()))?;
    external_command(root, &toml::Value::Table(raw))?;
    toml::from_str(text).map_err(|error| invalid(error.to_string()))
}

fn released_packages(
    root: &str,
    document: &Document,
) -> Result<Vec<(String, Settings)>, ReleaseError> {
    let mut released = Vec::new();
    for (index, entry) in document.package.iter().enumerate() {
        if document.package[..index]
            .iter()
            .any(|earlier| earlier.name == entry.name)
        {
            return Err(ReleaseError::DuplicatePackage {
                root: root.to_owned(),
                package: entry.name.clone(),
            });
        }
        if !valid_package_name(&entry.name) {
            return Err(ReleaseError::InvalidPackageName {
                root: root.to_owned(),
                package: entry.name.clone(),
            });
        }
        let effective = entry.settings.over(&document.workspace);
        if effective.release != Some(false) {
            released.push((entry.name.clone(), effective));
        }
    }
    Ok(released)
}

fn workspace_tags(
    root: &str,
    workspace: &Settings,
    nothing_listed: bool,
) -> Result<Vec<TagPattern>, ReleaseError> {
    if workspace.release == Some(false) {
        return if nothing_listed {
            Err(ReleaseError::RootReleasesNothing {
                root: root.to_owned(),
            })
        } else {
            Ok(Vec::new())
        };
    }
    let templates: Vec<&str> = match workspace.git_tag_name.as_deref() {
        Some(template) => vec![template],
        None if nothing_listed => {
            return Err(ReleaseError::MissingGitTagName {
                root: root.to_owned(),
                package: None,
            });
        }
        None => DEFAULT_WORKSPACE_TAGS.to_vec(),
    };
    check_package(root, None, workspace)?;
    templates
        .into_iter()
        .map(|template| checked(template, PACKAGE_WILDCARD, root, None))
        .collect()
}

pub(crate) fn read(root: &str, text: &str) -> Result<RootConfig, ReleaseError> {
    let document = parse_document(root, text)?;
    let workspace = &document.workspace;
    if workspace.release_always != Some(false) {
        return Err(ReleaseError::ReleaseAlwaysNotFalse {
            root: root.to_owned(),
        });
    }
    let prefix = workspace
        .pr_branch_prefix
        .clone()
        .unwrap_or_else(|| DEFAULT_PR_BRANCH_PREFIX.to_owned());
    if prefix.trim().is_empty() {
        return Err(ReleaseError::EmptyPrBranchPrefix {
            root: root.to_owned(),
        });
    }
    let released = released_packages(root, &document)?;
    let workspace_tags = workspace_tags(root, workspace, released.is_empty())?;
    let mut packages = Vec::new();
    for (name, settings) in &released {
        let Some(template) = settings.git_tag_name.as_deref() else {
            return Err(ReleaseError::MissingGitTagName {
                root: root.to_owned(),
                package: Some(name.clone()),
            });
        };
        let tag_pattern = checked(template, name, root, Some(name))?;
        check_package(root, Some(name), settings)?;
        packages.push(Package {
            name: name.clone(),
            tag_pattern,
        });
    }
    let publishes_to_crates_io = publishes(workspace, &released);
    Ok(RootConfig {
        root: root.to_owned(),
        publishes_to_crates_io,
        pr_branch_prefix: prefix,
        packages,
        workspace_tags,
    })
}
