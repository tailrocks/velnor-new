// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::ReleaseError;
use crate::settings::{Document, Settings};
use crate::tag::{TagPattern, checked};

const DEFAULT_PR_BRANCH_PREFIX: &str = "release-plz-";

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
}

fn check_package(
    root: &str,
    package: Option<&str>,
    settings: &Settings,
) -> Result<(), ReleaseError> {
    let owned = || (root.to_owned(), package.map(str::to_owned));
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
    if released.is_empty() {
        workspace.publishes_to_crates_io()
    } else {
        released
            .iter()
            .any(|(_, settings)| settings.publishes_to_crates_io())
    }
}

pub(crate) fn read(root: &str, text: &str) -> Result<RootConfig, ReleaseError> {
    let document: Document = toml::from_str(text).map_err(|error| ReleaseError::InvalidToml {
        file: format!("{root}/release-plz.toml"),
        message: error.to_string(),
    })?;
    let workspace = &document.workspace;
    if workspace.release_always != Some(false) {
        return Err(ReleaseError::ReleaseAlwaysNotFalse {
            root: root.to_owned(),
        });
    }
    let mut released: Vec<(String, Settings)> = Vec::new();
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
        let effective = entry.settings.over(workspace);
        if effective.release != Some(false) {
            released.push((entry.name.clone(), effective));
        }
    }
    if released.is_empty() {
        if workspace.release == Some(false) {
            return Err(ReleaseError::RootReleasesNothing {
                root: root.to_owned(),
            });
        }
        if workspace.git_tag_name.is_none() {
            return Err(ReleaseError::MissingGitTagName {
                root: root.to_owned(),
                package: None,
            });
        }
        checked(
            workspace.git_tag_name.as_deref().unwrap_or_default(),
            "package",
            root,
            None,
        )?;
        check_package(root, None, workspace)?;
    }
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
        pr_branch_prefix: workspace
            .pr_branch_prefix
            .clone()
            .unwrap_or_else(|| DEFAULT_PR_BRANCH_PREFIX.to_owned()),
        packages,
    })
}
