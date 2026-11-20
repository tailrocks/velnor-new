// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use crate::error::Reason;

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

    fn fmt_inputs(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
            Self::LockLine(line) => write!(f, "line {line} is not `path<TAB>package` or `path<TAB>package<TAB>manifest`"),
            Self::UnknownLockedPackage(package) => write!(
                f,
                "lists {package}, which is not a package of this repository; run `velnor-ci inputs audit --write` after a build of all targets"
            ),
            Self::UnknownRoot(root) => write!(
                f,
                "has times for {root}, which is not a Cargo root of this repository; run `velnor-ci timings` again"
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
            Self::PrCiWithSkipDraft => f.write_str(
                "pr_ci = \"label\" and skip_draft_pull_requests = true both decide what a draft pull request runs; remove skip_draft_pull_requests, because pr_ci = \"label\" already runs nothing on a draft",
            ),
            Self::MbxRemotePrivate(cause) => write!(
                f,
                "cache.mbx_remote is refused: fork pull requests can restore that cache and this repository has private dependencies ({cause}); remove cache.mbx_remote"
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
            Self::LockedUrlCredentials(name) => write!(
                f,
                "{name} is locked from an https URL with credentials in it, which the private_sources token cannot replace, so git could not fetch it; lock it from a plain https URL (the token supplies the credentials) or an ssh URL"
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
            Self::RustdocflagsString => f.write_str(
                "rustdoc flags must be set as an array in build.rustdocflags, not as a string and not per target: the generated documentation step adds -D warnings there with --config",
            ),
            Self::CargoConfigInclude => f.write_str(
                "include is not supported: velnor-gen must see every cargo setting that reaches the generated steps, so move the included settings into this file",
            ),
            _ => Ok(()),
        }
    }

    fn fmt_mise(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MiseOverridesFlags(name) => write!(
                f,
                "{name} is not supported: mise-action installs tools and exports this configuration's environment to every step, where plugins or variables can replace or weaken a check; set cargo and rustup settings in the cargo configuration or the toolchain file"
            ),
            Self::MiseTemplate => f.write_str(
                "templates are not supported: mise evaluates them while installing tools and can execute commands that replace or weaken a check; use literal configuration values"
            ),
            Self::OtherMiseConfig => f.write_str(
                "is a mise configuration that mise-action loads too; velnor-gen checks only mise.toml and .mise.toml, so move its settings there",
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
            Self::Hosting(error) => error.fmt(f),
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
            Self::OverlappingRules { .. }
            | Self::UnknownInputPackage(_)
            | Self::AmbiguousNextestHarness(_)
            | Self::UnknownNextestHarness(_)
            | Self::UntrackedManifest
            | Self::LockLine(_)
            | Self::UnknownLockedPackage(_)
            | Self::UnknownRoot(_) => self.fmt_inputs(f),
            Self::UnknownTestGroupRoot(_)
            | Self::TestGroupWithoutNextest(_)
            | Self::TestGroupFilter(_)
            | Self::TestGroupsWithPrivateSources => self.fmt_test_groups(f),
            Self::UnknownTestLaneRoot(_)
            | Self::TestLaneWithoutNextest(_)
            | Self::UnknownTestLanePackage(_)
            | Self::TestLaneFeatureConflict { .. } => self.fmt_test_lane(f),
            Self::UnknownTestSetupRoot(root) => write!(
                f,
                "test_setup names the root {root}, which is not a Cargo root of this repository; name the folder of a root's Cargo.toml, or . for the repository root"
            ),
            Self::RustdocflagsString => f.write_str(
                "rustdoc flags must be set as an array in build.rustdocflags, not as a string and not per target: the generated documentation step adds -D warnings there with --config",
            ),
            Self::MiseOverridesFlags(_) | Self::MiseTemplate | Self::OtherMiseConfig => {
                self.fmt_mise(f)
            }
            Self::UndeclaredTarget(_) | Self::HostTupleVersion(_) => self.fmt_build_target(f),
            Self::CargoEnv(name) => write!(
                f,
                "[env] sets {name}: cargo passes it to the compilers and clippy the checks run, and with force = true it replaces what the step sets, so a check could pass without checking; remove it, or set it without force and with a name that does not start with CLIPPY, RUST or CARGO"
            ),
            Self::UnpinnedTool(_)
            | Self::HelperBox(_)
            | Self::MbxRemotePrivate(_)
            | Self::PrCiWithSkipDraft => self.fmt_setup(f),
            Self::BuildExecutable
            | Self::CargoCredentialProvider(_)
            | Self::LockedUrlCredentials(_)
            | Self::CapLints
            | Self::TargetDirOutside(_)
            | Self::TargetDirOverlap(_)
            | Self::BuildDirKey(_)
            | Self::RuntimeTargetDir { .. }
            | Self::CargoConfigInclude => self.fmt_cargo_config(f),
            Self::DuplicateGroup(_)
            | Self::GroupRootTwice { .. }
            | Self::UnknownGroupRoot(_)
            | Self::GroupNameTaken(_)
            | Self::GroupHelper
            | Self::GroupShape
            | Self::GroupTestGroups(_)
            | Self::Lint(_)
            | Self::LintDuplicate(_)
            | Self::LintKeyForeign { .. } => self.fmt_config(f),
            Self::InvalidPatch(key) => {
                write!(
                    f,
                    "{key} must be a table whose path is a folder in the repository"
                )
            }
        }
    }
}
