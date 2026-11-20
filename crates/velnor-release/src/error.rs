// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
    InvalidProfile(String, String),
    UndefinedProfile(String, String, String),
    InvalidAssetName {
        package: String,
        template: String,
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
    KeyNeedsFormat {
        package: String,
        key: &'static str,
        format: &'static str,
    },
    MuslNeedsLinux {
        package: String,
    },
    InvalidCheck(String, crate::check::CheckProblem),
    InvalidBundle {
        package: String,
        entry: String,
        reason: &'static str,
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
    Shipped(crate::shipped::ShippedError),
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
    Family(crate::error_family::FamilyError),
    Group(crate::groups::GroupError),
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
