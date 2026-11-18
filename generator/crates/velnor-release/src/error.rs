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
    VersionEndsInLock(String, String),
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
    EmptyPrBranchPrefix {
        root: String,
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
    ExecutableConfig {
        root: String,
        key: &'static str,
    },
    ReleasePlzToolOverride {
        tool: String,
        field: String,
    },
    MiseRedirectsInstall {
        file: String,
        key: String,
    },
    ReleasePlzArtifactElsewhere {
        tool: String,
        url: String,
    },
    CargoCredentialProvider {
        file: String,
    },
    CargoRedirectsCratesIo {
        file: String,
    },
    CargoOffline {
        file: String,
    },
    PublishNamesOtherRegistry {
        root: String,
        manifest: String,
        registry: String,
    },
    ManifestUnreadable {
        manifest: String,
    },
    RootWithoutWorkspace {
        root: String,
    },
    CargoConfigInclude {
        file: String,
    },
    CargoConfigKeyNotAllowed {
        file: String,
        key: String,
    },
    SymlinkOnCargoConfigPath {
        root: String,
        path: String,
    },
    ToolchainFromPath {
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
    ExpressionInValue {
        key: String,
        value: String,
    },
    InvalidPrBranchPrefix {
        root: String,
        prefix: String,
    },
    AfterMissingWorkflow {
        name: String,
    },
    NightlyOnly {
        file: String,
        what: &'static str,
    },
    ReleasePlzVersionNotExact {
        version: String,
    },
    InvalidPublishTimeout {
        root: String,
        value: String,
    },
    PackagePublishTimeout {
        root: String,
        package: String,
    },
    PublishBudgetOutlivesToken {
        minutes: u64,
        limit: u64,
    },
    ReleasePlzVersionTooOld {
        version: String,
        minimum: String,
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
    NestedReleasePlzRoot {
        root: String,
    },
    UnknownWorkflow {
        name: String,
    },
    ReleasePlzNotInMiseToml,
    ReleasePlzNotInMiseLock,
    ReleasePlzBackendNotDownloadable {
        tool: String,
    },
    ReleasePlzNotLockedForLinux {
        tool: String,
    },
    ReleasePlzVersionsConflict {
        first: String,
        second: String,
    },
    ReleasePlzLockMismatch {
        configured: String,
        locked: Vec<String>,
    },
}

pub(crate) fn subject(package: Option<&String>) -> String {
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
            Self::ReleasePlzBackendNotDownloadable { tool } => write!(
                f,
                "mise.toml: {tool} builds release-plz from source and has no GitHub release archive to verify; use `release-plz`, `aqua:release-plz/release-plz` or `github:release-plz/release-plz`"
            ),
            Self::ReleasePlzNotLockedForLinux { tool } => write!(
                f,
                "mise.lock: {tool} has no platforms.linux-x64 entry with a sha256 checksum and a release-plz GitHub release url, so the release job cannot verify the download; run `mise lock` again"
            ),
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
            Self::EmptyPrBranchPrefix { root } => write!(
                f,
                "{root}/release-plz.toml: pr_branch_prefix must not be empty or blank"
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
            Self::ExecutableConfig { root, key } => write!(
                f,
                "{root}/release-plz.toml: {key} can run commands or read a file that Velnor does not check, and release-plz holds the release App token"
            ),
            Self::ReleasePlzToolOverride { tool, field } => write!(
                f,
                "mise.toml: the release-plz tool {tool} sets {field}, which can point mise at repository code or run a script while the release App token is in scope; give it only a version"
            ),
            Self::MiseRedirectsInstall { file, key } => write!(
                f,
                "{file}: {key} can redirect where mise downloads release-plz from, so the downloaded program runs with the release App token in scope; remove it"
            ),
            Self::ReleasePlzArtifactElsewhere { tool, url } => write!(
                f,
                "mise.lock: {tool} downloads from {url}, which is not the release-plz archive of the configured version, so the downloaded program would run with the release App token in scope; run `mise lock` again"
            ),
            _ => self.fmt_cargo(f),
        }
    }
}

impl Error for ReleaseError {}
