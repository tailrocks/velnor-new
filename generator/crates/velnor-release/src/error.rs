// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseError {
    InvalidToml {
        file: String,
        message: String,
    },
    MissingReleasePlzToml {
        root: String,
    },
    MissingGitTagName {
        root: String,
        package: Option<String>,
    },
    InvalidTagTemplate {
        root: String,
        package: Option<String>,
        template: String,
    },
    GitReleaseDraftNotTrue {
        root: String,
        package: Option<String>,
    },
    ReleaseAlwaysNotFalse {
        root: String,
    },
    SemverCheckNotFalse {
        root: String,
        package: Option<String>,
    },
    PublishWithoutNoVerify {
        root: String,
        package: Option<String>,
    },
    AmbiguousBinary {
        package: String,
        roots: Vec<String>,
    },
    UnknownPlatform {
        package: String,
        platform: String,
    },
    EmptyAfter,
    EmptyRoots,
    EmptyPlatforms {
        package: String,
    },
    UnsafeRoot {
        root: String,
    },
    DuplicatePackage {
        root: String,
        package: String,
    },
    InvalidPackageName {
        root: String,
        package: String,
    },
    RootReleasesNothing {
        root: String,
    },
    DuplicateTagPattern {
        pattern: String,
        packages: Vec<String>,
    },
    DuplicatePrBranchPrefix {
        prefix: String,
        roots: Vec<String>,
    },
    BinaryNotReleased {
        package: String,
    },
    TagTemplateWithoutVersion {
        root: String,
        package: Option<String>,
    },
    ReleaseSwitchOff {
        root: String,
        package: Option<String>,
        field: &'static str,
    },
    PublishWithGitOnly {
        root: String,
        package: Option<String>,
    },
    EmptyPrBranchPrefix {
        root: String,
    },
    ReleasePlzNotInMiseToml,
    ReleasePlzNotInMiseLock,
    ReleasePlzLockMismatch {
        configured: String,
        locked: Vec<String>,
    },
}

fn subject(package: Option<&String>) -> String {
    package.map_or_else(
        || "the [workspace] table".to_owned(),
        |name| format!("package {name}"),
    )
}

impl fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidToml { file, message } => write!(f, "{file} is not valid: {message}"),
            Self::MissingReleasePlzToml { root } => {
                write!(f, "root {root} has no release-plz.toml")
            }
            Self::MissingGitTagName { root, package } => write!(
                f,
                "{root}/release-plz.toml: {} has no explicit git_tag_name",
                subject(package.as_ref())
            ),
            Self::InvalidTagTemplate {
                root,
                package,
                template,
            } => write!(
                f,
                "{root}/release-plz.toml: git_tag_name {template:?} of {} may use only literals, {{{{ package }}}} and {{{{ version }}}}",
                subject(package.as_ref())
            ),
            Self::GitReleaseDraftNotTrue { root, package } => write!(
                f,
                "{root}/release-plz.toml: git_release_draft must be true for {}",
                subject(package.as_ref())
            ),
            Self::ReleaseAlwaysNotFalse { root } => {
                write!(f, "{root}/release-plz.toml: release_always must be false")
            }
            Self::SemverCheckNotFalse { root, package } => write!(
                f,
                "{root}/release-plz.toml: semver_check must be false for {}",
                subject(package.as_ref())
            ),
            Self::PublishWithoutNoVerify { root, package } => write!(
                f,
                "{root}/release-plz.toml: {} publishes to crates.io and needs publish_no_verify = true",
                subject(package.as_ref())
            ),
            Self::DuplicateTagPattern { pattern, packages } => write!(
                f,
                "packages {} share the tag pattern {pattern}",
                packages.join(", ")
            ),
            Self::DuplicatePrBranchPrefix { prefix, roots } => write!(
                f,
                "roots {} share pr_branch_prefix {prefix:?}",
                roots.join(", ")
            ),
            _ => self.fmt_config(f),
        }
    }
}

impl ReleaseError {
    fn fmt_config(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BinaryNotReleased { package } => write!(
                f,
                ".velnor/release.toml: binary {package} is not a released [[package]] in any release-plz.toml"
            ),
            Self::AmbiguousBinary { package, roots } => write!(
                f,
                ".velnor/release.toml: binary {package} is released from several roots: {}",
                roots.join(", ")
            ),
            Self::UnknownPlatform { package, platform } => write!(
                f,
                ".velnor/release.toml: binary {package} names unknown platform {platform:?}"
            ),
            Self::EmptyAfter => write!(
                f,
                ".velnor/release.toml: after must name at least one non-blank workflow"
            ),
            Self::EmptyPlatforms { package } => write!(
                f,
                ".velnor/release.toml: binary {package} must name at least one platform"
            ),
            Self::UnsafeRoot { root } => write!(
                f,
                ".velnor/release.toml: root {root:?} must be a relative path without \"..\""
            ),
            Self::DuplicatePackage { root, package } => write!(
                f,
                "{root}/release-plz.toml: package {package} has more than one [[package]] entry"
            ),
            Self::InvalidPackageName { root, package } => write!(
                f,
                "{root}/release-plz.toml: {package:?} is not a valid package name"
            ),
            Self::RootReleasesNothing { root } => write!(
                f,
                "{root}/release-plz.toml: [workspace] release = false leaves nothing to release"
            ),
            Self::TagTemplateWithoutVersion { root, package } => write!(
                f,
                "{root}/release-plz.toml: git_tag_name of {} must contain {{{{ version }}}} so every release gets its own tag",
                subject(package.as_ref())
            ),
            Self::ReleaseSwitchOff {
                root,
                package,
                field,
            } => write!(
                f,
                "{root}/release-plz.toml: {field} must not be false for {}",
                subject(package.as_ref())
            ),
            Self::PublishWithGitOnly { root, package } => write!(
                f,
                "{root}/release-plz.toml: {} sets publish = true together with git_only = true, which release-plz refuses",
                subject(package.as_ref())
            ),
            Self::EmptyPrBranchPrefix { root } => write!(
                f,
                "{root}/release-plz.toml: pr_branch_prefix must not be empty"
            ),
            Self::EmptyRoots => {
                write!(f, ".velnor/release.toml: roots must name at least one root")
            }
            Self::ReleasePlzNotInMiseToml => write!(f, "mise.toml does not list release-plz"),
            Self::ReleasePlzNotInMiseLock => write!(f, "mise.lock does not lock release-plz"),
            Self::ReleasePlzLockMismatch { configured, locked } => write!(
                f,
                "mise.toml requests release-plz {configured} but mise.lock locks {}",
                locked.join(", ")
            ),
            _ => Ok(()),
        }
    }
}

impl Error for ReleaseError {}
