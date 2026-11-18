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
    RustdocflagsString,
    CargoConfigInclude,
    MiseOverridesFlags(String),
    OtherMiseConfig,
    CapLints,
    CargoEnv(String),
    UndeclaredTarget(String),
    HostTupleVersion(String),
    BuildExecutable,
    UnpinnedTool(&'static str),
    HelperBox(RepoPath),
    HelperSourceRelease,
    CargoCredentialProvider(String),
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

impl Reason {
    fn fmt_build_target(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndeclaredTarget(target) => write!(
                f,
                "build.target names {target}, which rust-toolchain.toml does not list in targets; the generated jobs install only the declared targets"
            ),
            Self::HostTupleVersion(version) => write!(
                f,
                "build.target uses host-tuple, which Cargo does not support in Rust {version}; use Rust 1.91.0 or newer"
            ),
            _ => Ok(()),
        }
    }

    fn fmt_setup(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnpinnedTool(tool) => write!(
                f,
                "is present but {tool} is not pinned in mise.toml, so the policy lane could not run it; pin it there"
            ),
            Self::HelperBox(path) => write!(
                f,
                "helper.path is {path}, which is not a Cargo root with the package velnor-ci"
            ),
            Self::HelperSourceRelease => f.write_str(
                "helper.path requires velnor-ci 0.1.81 or newer for input restoration and cache tie-breaking; run `velnor-gen pin-helper 0.1.81`",
            ),
            _ => Ok(()),
        }
    }

    fn fmt_cargo_config(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuildExecutable => f.write_str(
                "build.rustc, build.rustdoc and the build wrappers replace the programs the checks run; remove them",
            ),
            Self::CargoCredentialProvider(key) => write!(
                f,
                "{key} names a program or alias that cargo runs when it publishes, with the release token in its environment; remove it, or use one of cargo:token, cargo:wincred, cargo:macos-keychain or cargo:libsecret"
            ),
            Self::CapLints => f.write_str(
                "rustflags or rustdocflags with --cap-lints, --force-warn or an @file argument could let the lint or documentation step pass with warnings; remove it",
            ),
            Self::TargetDirOutside(value) => write!(
                f,
                "build.target-dir is {value:?}, which is not a plain folder inside the repository below its root (a leading ~ or a {{placeholder}} is not modelled); the target cache can only save a folder of the checkout, so name one"
            ),
            Self::TargetDirOverlap(value) => write!(f, "build.target-dir {value:?} overlaps tracked checkout content or repository control paths; use an untracked build directory"),
            Self::RuntimeTargetDir { used, rendered } => write!(
                f,
                "puts the target directory at {used}, but the generated workflow caches {rendered}, so cargo would write outside the cached target path: restore the generated target directory, or commit the change and regenerate"
            ),
            Self::BuildDirKey(key) => write!(
                f,
                "{key} is not supported: the target cache cannot model a separate build directory, so remove it"
            ),
            Self::CargoConfigInclude => f.write_str(
                "include is not supported: velnor-gen must see every cargo setting that reaches the generated steps, so move the included settings into this file",
            ),
            _ => Ok(()),
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotUtf8 => f.write_str("is not UTF-8"),
            Self::Syntax(message) => f.write_str(message.trim_end()),
            Self::UnknownKey(key) => write!(f, "unknown key {key}"),
            Self::MissingTable(table) => write!(f, "missing table [{table}]"),
            Self::MissingKey(key) => write!(f, "missing key {key}"),
            Self::InvalidValue { key, value } => write!(f, "invalid value {value:?} for {key}"),
            Self::OutOfRange {
                key,
                value,
                min,
                max,
            } => write!(f, "{key} must be between {min} and {max}, got {value}"),
            Self::UnmatchedExclude(glob) => {
                write!(f, "exclude entry {glob} matches no tracked Cargo.toml")
            }
            Self::HelperLock(error) => error.fmt(f),
            Self::OverlappingRules { non_input, input } => write!(
                f,
                "non_inputs entry {non_input} overlaps inputs entry {input}; a path cannot select nothing and select packages"
            ),
            Self::UnknownInputPackage(package) => write!(
                f,
                "inputs names {package}, which is not a package of this repository"
            ),
            Self::AmbiguousNextestHarness(entry) => write!(
                f,
                "nextest_harnesses names {entry}, which more than one test target declares"
            ),
            Self::UnknownNextestHarness(entry) => write!(
                f,
                "nextest_harnesses names {entry}, which is not a test target with harness = false"
            ),
            Self::UntrackedManifest => f.write_str(
                "is a workspace member that git does not track; run `git add` on it so its harness = false tests are seen",
            ),
            Self::LockLine(line) => write!(f, "line {line} is not `path<TAB>package`"),
            Self::UnknownLockedPackage(package) => write!(
                f,
                "lists {package}, which is not a package of this repository; run `velnor-ci inputs audit --write` after a build of all targets"
            ),
            Self::UnknownRoot(root) => write!(
                f,
                "has times for {root}, which is not a Cargo root of this repository; run `velnor-ci timings` again"
            ),
            Self::UnknownTestSetupRoot(root) => write!(
                f,
                "test_setup names the root {root}, which is not a Cargo root of this repository; name the folder of a root's Cargo.toml, or . for the repository root"
            ),
            Self::RustdocflagsString => f.write_str(
                "rustdoc flags must be set as an array in build.rustdocflags, not as a string and not per target: the generated documentation step adds -D warnings there with --config",
            ),
            Self::MiseOverridesFlags(name) => write!(
                f,
                "{name} is not supported: mise-action exports this configuration's environment to every step, where variables can replace or weaken a check; set cargo and rustup settings in the cargo configuration or the toolchain file"
            ),
            Self::UndeclaredTarget(_) | Self::HostTupleVersion(_) => self.fmt_build_target(f),
            Self::CargoEnv(name) => write!(
                f,
                "[env] sets {name}: cargo passes it to the compilers and clippy the checks run, and with force = true it replaces what the step sets, so a check could pass without checking; remove it, or set it without force and with a name that does not start with CLIPPY, RUST or CARGO"
            ),
            Self::OtherMiseConfig => f.write_str(
                "is a mise configuration that mise-action loads too; velnor-gen checks only mise.toml and .mise.toml, so move its settings there",
            ),
            Self::UnpinnedTool(_) | Self::HelperBox(_) | Self::HelperSourceRelease => self.fmt_setup(f),
            Self::BuildExecutable
            | Self::CargoCredentialProvider(_)
            | Self::CapLints
            | Self::TargetDirOutside(_)
            | Self::TargetDirOverlap(_)
            | Self::BuildDirKey(_)
            | Self::RuntimeTargetDir { .. }
            | Self::CargoConfigInclude => self.fmt_cargo_config(f),
            Self::InvalidPatch(key) => {
                write!(
                    f,
                    "{key} must be a table whose path is a folder in the repository"
                )
            }
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
