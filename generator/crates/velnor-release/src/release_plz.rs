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
    if let Some(package) = package
        && settings.publish != Some(false)
        && settings.publish_no_verify != Some(true)
    {
        return Err(ReleaseError::PublishWithoutNoVerify {
            root: root.to_owned(),
            package: package.to_owned(),
        });
    }
    Ok(())
}

pub(crate) fn read(
    root: &str,
    text: &str,
    extra_packages: &[&str],
) -> Result<RootConfig, ReleaseError> {
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
    for entry in &document.package {
        let effective = entry.settings.over(workspace);
        if effective.release != Some(false) {
            released.push((entry.name.clone(), effective));
        }
    }
    for name in extra_packages {
        if !released.iter().any(|(existing, _)| existing == name) {
            released.push(((*name).to_owned(), workspace.clone()));
        }
    }
    if released.is_empty() {
        if workspace.git_tag_name.is_none() {
            return Err(ReleaseError::MissingGitTagName {
                root: root.to_owned(),
                package: None,
            });
        }
        checked(
            workspace.git_tag_name.as_deref().unwrap_or_default(),
            "",
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
    Ok(RootConfig {
        root: root.to_owned(),
        pr_branch_prefix: workspace
            .pr_branch_prefix
            .clone()
            .unwrap_or_else(|| DEFAULT_PR_BRANCH_PREFIX.to_owned()),
        packages,
    })
}
