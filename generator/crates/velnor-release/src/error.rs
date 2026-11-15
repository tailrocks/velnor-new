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
    UnknownPlatform {
        package: String,
        platform: String,
    },
    InvalidBinName {
        package: String,
        bin: String,
    },
    EmptyAfter,
    EmptyRoots,
    EmptyPlatforms {
        package: String,
    },
    UnsafeRoot {
        root: String,
    },
    DuplicateBinary {
        package: String,
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
    GitTagDisabled {
        root: String,
        package: Option<String>,
    },
    GitReleaseDisabled {
        root: String,
        package: Option<String>,
    },
    EmptyPrBranchPrefix {
        root: String,
    },
    ExternalCommand {
        root: String,
        key: String,
    },
    CargoCredentialProvider {
        file: String,
    },
    DuplicatePackageName {
        package: String,
        roots: Vec<String>,
    },
    TagsMayCollide {
        first: String,
        second: String,
    },
    CargoRunsProgram {
        file: String,
        key: String,
    },
    ReleasePlzVersionNotExact {
        version: String,
    },
    PackageNotInRoot {
        root: String,
        package: String,
    },
    BinaryTargetMissing {
        package: String,
        bin: String,
    },
    RootWithoutCargoManifest {
        root: String,
    },
    UnknownWorkflow {
        name: String,
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
            Self::UnknownPlatform { package, platform } => write!(
                f,
                ".velnor/release.toml: binary {package} names unknown platform {platform:?}"
            ),
            Self::InvalidBinName { package, bin } => write!(
                f,
                ".velnor/release.toml: binary {package} names bin {bin:?}, which is not a valid binary name"
            ),
            Self::EmptyAfter => write!(
                f,
                ".velnor/release.toml: after must name at least one non-blank workflow"
            ),
            Self::EmptyPlatforms { package } => write!(
                f,
                ".velnor/release.toml: binary {package} must name at least one platform"
            ),
            Self::DuplicateBinary { package } => write!(
                f,
                ".velnor/release.toml: package {package} has more than one [[binaries]] entry"
            ),
            Self::UnsafeRoot { root } => write!(
                f,
                ".velnor/release.toml: root {root:?} must be \".\" or a relative path without empty, \".\" or \"..\" components"
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
            _ => self.fmt_hardening(f),
        }
    }

    fn fmt_hardening(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GitTagDisabled { root, package } => write!(
                f,
                "{root}/release-plz.toml: git_tag_enable must not be false for {}",
                subject(package.as_ref())
            ),
            Self::GitReleaseDisabled { root, package } => write!(
                f,
                "{root}/release-plz.toml: git_release_enable must not be false for {}",
                subject(package.as_ref())
            ),
            Self::EmptyPrBranchPrefix { root } => write!(
                f,
                "{root}/release-plz.toml: pr_branch_prefix must not be empty or blank"
            ),
            Self::ExternalCommand { root, key } => write!(
                f,
                "{root}/release-plz.toml: {key} runs an external command with the release token in reach"
            ),
            Self::CargoCredentialProvider { file } => write!(
                f,
                "{file}: a cargo credential provider would run next to the crates.io publish token"
            ),
            Self::CargoRunsProgram { file, key } => write!(
                f,
                "{file}: {key} makes cargo run a program while release-plz holds the release App token"
            ),
            Self::ReleasePlzVersionNotExact { version } => write!(
                f,
                "mise.toml: release-plz must be pinned to an exact MAJOR.MINOR.PATCH version, not {version:?}"
            ),
            Self::RootWithoutCargoManifest { root } => write!(
                f,
                ".velnor/release.toml: root {root} has no tracked Cargo.toml"
            ),
            Self::UnknownWorkflow { name } => write!(
                f,
                ".velnor/release.toml: after names workflow {name:?}, which no tracked or generated workflow has as its name"
            ),
            Self::PackageNotInRoot { root, package } => write!(
                f,
                "{root}/release-plz.toml: [[package]] {package} is not a package of the Cargo workspace at {root}"
            ),
            Self::BinaryTargetMissing { package, bin } => write!(
                f,
                ".velnor/release.toml: package {package} has no binary target {bin} that builds without extra features in its release root"
            ),
            Self::DuplicatePackageName { package, roots } => write!(
                f,
                "package {package} is released from several roots: {}",
                roots.join(", ")
            ),
            Self::TagsMayCollide { first, second } => write!(
                f,
                "tag patterns {first} and {second} can render the same tag"
            ),
            _ => Ok(()),
        }
    }
}

impl Error for ReleaseError {}
