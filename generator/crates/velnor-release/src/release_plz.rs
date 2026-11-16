// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::error::ReleaseError;
use crate::git_ref;
use crate::settings::{Document, Settings, executable_key};
use crate::tag::{PACKAGE_WILDCARD, TagPattern, checked};
use crate::timeout;

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
    pub(crate) workspace_tags: Vec<TagPattern>,
    entries: BTreeMap<String, bool>,
    unreleased: BTreeSet<String>,
    unlisted_publish: bool,
    pub(crate) listed_minutes: u64,
    pub(crate) unlisted_publish_minutes: u64,
    pub(crate) unlisted_check_minutes: u64,
}

impl RootConfig {
    pub(crate) fn releases(&self, package: &str) -> bool {
        !self.unreleased.contains(package)
    }

    pub(crate) fn settle_publishing(&mut self, unlisted_member: bool) {
        self.publishes_to_crates_io = self.entries.values().any(|publishes| *publishes)
            || (self.unlisted_publish && unlisted_member);
    }

    pub(crate) fn publishes(&self, package: &str) -> bool {
        self.publishes_to_crates_io
            && self
                .entries
                .get(package)
                .copied()
                .unwrap_or(self.unlisted_publish)
    }
}

fn check_package(
    root: &str,
    package: Option<&str>,
    settings: &Settings,
) -> Result<(), ReleaseError> {
    if let Some(value) = &settings.publish_timeout
        && timeout::minutes(value).is_none()
    {
        return Err(ReleaseError::InvalidPublishTimeout {
            root: root.to_owned(),
            value: value.clone(),
        });
    }
    let owned = || (root.to_owned(), package.map(str::to_owned));
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
    for (field, value) in [
        ("git_tag_enable", settings.git_tag_enable),
        ("git_release_enable", settings.git_release_enable),
    ] {
        if value == Some(false) {
            let (root, package) = owned();
            return Err(ReleaseError::ReleaseSwitchOff {
                root,
                package,
                field,
            });
        }
    }
    if settings.publish == Some(true) && settings.git_only == Some(true) {
        let (root, package) = owned();
        return Err(ReleaseError::PublishWithGitOnly { root, package });
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
    let invalid = |error: toml::de::Error| ReleaseError::InvalidToml {
        file: format!("{root}/release-plz.toml"),
        message: error.to_string(),
    };
    let document: Document = toml::from_str(text).map_err(invalid)?;
    let raw: toml::Value = toml::from_str(text).map_err(invalid)?;
    match executable_key(&raw) {
        Some(key) => Err(ReleaseError::ExecutableConfig {
            root: root.to_owned(),
            key,
        }),
        None => refuse_package_publish_timeout(root, &document).map(|()| document),
    }
}

pub fn root_publishes_to_crates_io(root: &str, text: &str) -> Result<bool, String> {
    let document = parse_document(root, text).map_err(|error| error.to_string())?;
    let released = released_packages(root, &document).map_err(|error| error.to_string())?;
    Ok(publishes(&document.workspace, &released))
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
    let Some(template) = workspace.git_tag_name.as_deref() else {
        return Err(ReleaseError::MissingGitTagName {
            root: root.to_owned(),
            package: None,
        });
    };
    let pattern = checked(template, PACKAGE_WILDCARD, root, None)?;
    check_package(root, None, workspace)?;
    Ok(vec![pattern])
}

fn refuse_package_publish_timeout(root: &str, document: &Document) -> Result<(), ReleaseError> {
    match document
        .package
        .iter()
        .find(|entry| entry.settings.publish_timeout.is_some())
    {
        Some(entry) => Err(ReleaseError::PackagePublishTimeout {
            root: root.to_owned(),
            package: entry.name.clone(),
        }),
        None => Ok(()),
    }
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
    if !git_ref::is_valid(&format!("refs/heads/{prefix}x")) {
        return Err(ReleaseError::InvalidPrBranchPrefix {
            root: root.to_owned(),
            prefix,
        });
    }
    let released = released_packages(root, &document)?;
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
    let workspace_tags = workspace_tags(root, workspace, released.is_empty())?;
    let publishes_to_crates_io = publishes(workspace, &released);
    let entries = document
        .package
        .iter()
        .map(|entry| {
            let effective = entry.settings.over(workspace);
            (
                entry.name.clone(),
                effective.release != Some(false) && effective.publishes_to_crates_io(),
            )
        })
        .collect();
    let unreleased = document
        .package
        .iter()
        .filter(|entry| entry.settings.over(workspace).release == Some(false))
        .map(|entry| entry.name.clone())
        .collect();
    let listed_minutes = released
        .iter()
        .map(|(_, settings)| settings.registry_check_and_publish_minutes())
        .sum();
    Ok(RootConfig {
        listed_minutes,
        unlisted_publish_minutes: workspace.publish_minutes(),
        unlisted_check_minutes: if workspace.release != Some(false)
            && workspace.publishes_to_crates_io()
        {
            workspace.publish_minutes()
        } else {
            0
        },
        entries,
        unreleased,
        unlisted_publish: workspace.release != Some(false) && workspace.publishes_to_crates_io(),
        root: root.to_owned(),
        publishes_to_crates_io,
        pr_branch_prefix: prefix,
        packages,
        workspace_tags,
    })
}

pub fn release_tag_patterns(root: &str, text: &str) -> Result<BTreeSet<String>, ReleaseError> {
    let config = read(root, text)?;
    Ok(config
        .packages
        .iter()
        .map(|package| &package.tag_pattern)
        .chain(&config.workspace_tags)
        .map(|pattern| pattern.as_str().to_owned())
        .collect())
}

pub(crate) fn root_of(file: &str) -> &str {
    match file.rsplit_once('/') {
        Some((root, _)) if !root.is_empty() => root,
        _ => ".",
    }
}
