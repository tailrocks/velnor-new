// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use velnor_model::{PathGlob, RepoPath};

use crate::config::CONFIG_PATH;
use crate::lock::LockError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectError {
    Invalid { path: RepoPath, reason: Reason },
    MissingLockfile { path: RepoPath, manifest: RepoPath },
    MiseRequired { path: RepoPath },
    MiseLockRequired { path: RepoPath },
    ToolNotPrebuilt { tool: String, platform: String },
    NextestRequired { path: RepoPath },
    MbxPinRequired { path: RepoPath },
    MbxLayout(crate::mbx_layout::Layout),
    MbxConfigSplit(crate::mbx_layout::Split),
    MbxSecondProvider(crate::mbx_layout::Provider),
    RustPinRequired { channel: String, path: RepoPath },
    SameSlug { first: RepoPath, second: RepoPath },
    ToolchainConflict { file: String, mise: String },
    NestedToolchain { file: RepoPath },
    NoRustRoot,
    JobIdTaken(String),
    UnhostedOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    NotUtf8,
    Syntax(String),
    UnknownKey(String),
    MissingTable(&'static str),
    MissingKey(&'static str),
    InvalidValue {
        key: &'static str,
        value: String,
    },
    OutOfRange {
        key: &'static str,
        value: u16,
        min: u16,
        max: u16,
    },
    UnmatchedExclude(PathGlob),
    HelperLock(LockError),
    InvalidPatch(String),
    OverlappingRules {
        non_input: PathGlob,
        input: PathGlob,
    },
    UnknownInputPackage(String),
    UnknownNextestHarness(String),
    AmbiguousNextestHarness(String),
    UntrackedManifest,
    LockLine(usize),
    UnknownLockedPackage(String),
    UnknownRoot(RepoPath),
    UnknownTestSetupRoot(String),
    UnknownCheckConfigurationsRoot(String),
    UnknownRootTimeoutRoot(String),
    Hosting(crate::config_hosting::HostingError),
    DuplicateGroup(String),
    GroupRootTwice {
        name: String,
        root: RepoPath,
    },
    UnknownGroupRoot(RepoPath),
    GroupNameTaken(String),
    GroupShape,
    GroupHelper,
    GroupTestGroups(RepoPath),
    UnknownTestLaneRoot(String),
    TestLaneWithoutNextest(String),
    UnknownTestLanePackage(String),
    TestLaneFeatureConflict {
        first: &'static str,
        second: &'static str,
    },
    Lint(crate::lint_error::LintFault),
    LintDuplicate(String),
    LintKeyForeign {
        kind: String,
        key: &'static str,
    },
    UnknownTestGroupRoot(String),
    TestGroupWithoutNextest(String),
    TestGroupFilter(String),
    TestGroupRestoreWithoutCargo(String),
    TestGroupsWithPrivateSources,
    PartitionsWithoutNextest(String),
    TestGroupArtifact {
        key: &'static str,
        value: String,
        fix: String,
    },
    ArtifactNameTaken(String),
    PrCiWithSkipDraft,
    RustdocflagsString,
    CargoConfigInclude,
    MiseOverridesFlags(String),
    MiseTemplate,
    OtherMiseConfig,
    CapLints,
    CargoEnv(String),
    UndeclaredTarget(String),
    HostTupleVersion(String),
    BuildExecutable,
    UnpinnedTool(&'static str),
    HelperBox(RepoPath),
    MbxRemotePrivate(String),
    MbxOffConflict(&'static str),
    CargoCredentialProvider(String),
    LockedUrlCredentials(String),
    TargetDirOutside(String),
    TargetDirOverlap(String),
    BuildDirKey(&'static str),
    RuntimeTargetDir {
        used: RepoPath,
        rendered: RepoPath,
    },
}

impl DetectError {
    pub(crate) fn invalid(path: &RepoPath, reason: Reason) -> Self {
        Self::Invalid {
            path: path.clone(),
            reason,
        }
    }
}

impl fmt::Display for DetectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { path, reason } => write!(f, "{path}: {reason}"),
            Self::MissingLockfile { path, manifest } => write!(
                f,
                "{path} is not committed; commit it, or add the directory of {manifest} to exclude in {CONFIG_PATH}"
            ),
            Self::MiseRequired { path } => write!(
                f,
                "a mise configuration is required; commit {path} with the tool versions the project uses"
            ),
            Self::MiseLockRequired { path } => write!(
                f,
                "{path} is required: the tools cache key is the hash of it, so a changed tool version could not change the key; run `mise lock` and commit it"
            ),
            Self::ToolNotPrebuilt { tool, platform } => write!(
                f,
                "mise.lock has no prebuilt binary of {tool} for {platform}; the tools cache holds prebuilt binaries only, so pin a tool that publishes one (or run `mise lock`) instead of building it from source"
            ),
            Self::NextestRequired { path } => write!(
                f,
                "the generated job runs the tests with cargo-nextest; pin it in {path}, for example \"aqua:nextest-rs/nextest/cargo-nextest\" = \"<version>\""
            ),
            Self::MbxPinRequired { path } => crate::mbx_layout::pin_required(path, f),
            Self::MbxLayout(layout) => layout.write(f),
            Self::MbxConfigSplit(split) => split.write(f),
            Self::MbxSecondProvider(provider) => provider.write(f),
            Self::RustPinRequired { channel, path } => write!(
                f,
                "the Rust toolchain file names {channel}, and the generated jobs never let rustup install a toolchain on its own; pin rust = \"{channel}\" in {path}"
            ),
            Self::SameSlug { first, second } => write!(
                f,
                "the Cargo roots {first} and {second} get the same job and output names; rename one of their folders or add one to exclude in {CONFIG_PATH}"
            ),
            Self::ToolchainConflict { file, mise } => write!(
                f,
                "the Rust toolchain file names {file} and mise pins rust {mise}; mise's pin wins in the generated job, so make them the same"
            ),
            Self::NestedToolchain { file } => write!(
                f,
                "{file} sets up tools for one Cargo root; the generated jobs set up one toolchain and one mise configuration for the repository, so move it to the repository root"
            ),
            Self::NoRustRoot => f.write_str("no Cargo workspace found outside the excluded paths"),
            Self::UnhostedOrigin => f.write_str(
                "origin must identify a hosted repository to place a hashed full_run schedule; point origin at a hosted URL, or set full_run to \"off\" or a cron expression",
            ),
            Self::JobIdTaken(id) => write!(
                f,
                "two generated jobs get the id {id}; rename the folder of a Cargo root or add it to exclude in {CONFIG_PATH}"
            ),
        }
    }
}

impl std::error::Error for DetectError {}
