// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fmt;

use crate::error::ReleaseError;

impl ReleaseError {
    pub(crate) fn fmt_cargo(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VersionEndsInLock(manifest, version) => write!(
                f,
                "{manifest}: version {version:?} would end a release tag in \".lock\", which git refuses"
            ),
            Self::CargoCredentialProvider { file } => write!(
                f,
                "{file}: a cargo credential provider or alias makes cargo run a program while release-plz holds the release App token"
            ),
            Self::SymlinkOnCargoConfigPath { root, path } => write!(
                f,
                "{path}: a tracked symbolic link on the cargo config path of release root {root} can hide a cargo config from velnor-gen while cargo follows it"
            ),
            Self::CargoRedirectsCratesIo { file } => write!(
                f,
                "{file}: a cargo proxy, CA override or non-crates.io default registry would redirect the crates.io publish token"
            ),
            Self::PublishNamesOtherRegistry {
                root,
                manifest,
                registry,
            } => write!(
                f,
                "{manifest}: package.publish names registry {registry}, so cargo would send the crates.io publish token of release root {root} to it; list only crates-io or set publish = false"
            ),
            Self::ManifestUnreadable { manifest } => write!(
                f,
                "{manifest}: the manifest cargo metadata lists is missing or does not parse, so its package.publish cannot be checked"
            ),
            Self::RootWithoutWorkspace { root } => write!(
                f,
                "{root}: no Cargo workspace known from cargo metadata owns this release root, so its packages and publish registries cannot be checked"
            ),
            Self::ToolchainFromPath { file } => write!(
                f,
                "{file}: a toolchain taken from a path makes rustup run repository code while release-plz holds the release App token"
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
            Self::ExpressionInValue { key, value } => write!(
                f,
                ".velnor/release.toml: {key} {value:?} contains \"${{{{\", which GitHub Actions would evaluate as an expression"
            ),
            Self::InvalidPrBranchPrefix { root, prefix } => write!(
                f,
                "{root}/release-plz.toml: pr_branch_prefix {prefix:?} cannot start a Git branch name"
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
            _ => self.fmt_cargo_config(f),
        }
    }

    fn fmt_cargo_config(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AfterMissingWorkflow { name } => write!(
                f,
                ".velnor/release.toml: workflow {name:?} runs on push and is not in after, so when it finishes last the gate has already answered ready=false and nothing starts Release again"
            ),
            Self::NightlyOnly { file, what } => write!(
                f,
                "{file}: {what} needs a nightly Rust, and the release jobs run release-plz and cargo on stable"
            ),
            Self::CargoConfigInclude { file } => write!(
                f,
                "{file}: a cargo config include loads a file velnor-gen never checked while release-plz holds the release App token"
            ),
            Self::CargoConfigKeyNotAllowed { file, key } => write!(
                f,
                "{file}: cargo config key {key} is not on the list known to be harmless while release-plz holds the release App token"
            ),
            Self::InvalidPublishTimeout { root, value } => write!(
                f,
                "{root}/release-plz.toml: publish_timeout {value:?} is not a duration such as \"10m\" or \"1h 30m\""
            ),
            Self::PackagePublishTimeout { root, package } => write!(
                f,
                "{root}/release-plz.toml: [[package]] {package} sets publish_timeout, which release-plz 0.4.3 reads only under [workspace], where it applies to every package; move it to [workspace]"
            ),
            Self::PublishBudgetOutlivesToken { minutes, limit } => write!(
                f,
                "the release-plz.toml publish_timeout of the released packages adds up to {minutes} minutes (release-plz applies it once to the registry check and once more to the publish of every released package that publishes, so each publishing package needs 2 x publish_timeout, and 30 minutes wherever none is set; packages with publish = false or git_only cost nothing), and the release App token that the tag job creates before publishing expires after 60 minutes, so at most {limit} minutes of publishing fit in one token; lower publish_timeout or release fewer packages per run (set publish_timeout under [workspace]; for example, with one publishing package set publish_timeout = \"10m\" or anything up to 25m, since 2 x 25m = 50m)"
            ),
            Self::ReleasePlzVersionTooOld { version, minimum } => write!(
                f,
                "mise.toml: release-plz {version} cannot authenticate the git fetch of a checkout without persisted credentials; pin release-plz {minimum} or newer"
            ),
            Self::CargoOffline { file } => write!(
                f,
                "{file}: net.offline stops cargo publish from reaching crates.io"
            ),
            Self::ReleasePlzVersionsConflict { first, second } => write!(
                f,
                "mise.toml selects release-plz {first} and {second}; every selector must resolve to one version"
            ),
            Self::InvalidCheck(check, problem) => {
                write!(f, ".velnor/release.toml: check {check:?} {problem}")
            }
            Self::Family(error) => write!(f, "{error}"),
            _ => self.fmt_binary(f),
        }
    }
}

impl Error for ReleaseError {}
