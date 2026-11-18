// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use velnor_model::{HelperVersion, RELEASE_CONFIG_PATH, RepoPath};
use velnor_release::ReleaseError;
use velnor_render::RuleViolation;
use velnor_rust::{DetectError, MetadataFileError};

use crate::{COMPATIBLE_HELPER_SERIES, RELEASE_HELPER_MINIMUM};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerateError {
    Detect(DetectError),
    IncompatibleHelper(HelperVersion),
    Release(ReleaseError),
    ReleaseFileNotUtf8(RepoPath),
    NoReleaseConfig,
    ReleaseToolchainPath,
    HelperNotPinned,
    ReleaseHelperTooOld(HelperVersion),
    MirrorHelperTooOld(HelperVersion),
    InputsBuildHelperTooOld(HelperVersion),
    PrOnlyChecksHelperTooOld(HelperVersion),
    BudgetHelperTooOld(HelperVersion),
    Metadata(MetadataFileError),
    Rules(Vec<RuleViolation>),
}

impl fmt::Display for GenerateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Detect(error) => error.fmt(f),
            Self::IncompatibleHelper(version) => write!(
                f,
                "velnor-ci {version} does not fit this velnor-gen, which works with velnor-ci {}.{}.x; pin a compatible release with `velnor-gen pin-helper <version>`",
                COMPATIBLE_HELPER_SERIES.0, COMPATIBLE_HELPER_SERIES.1
            ),
            Self::Release(error) => error.fmt(f),
            Self::ReleaseFileNotUtf8(path) => write!(f, "{path} is not UTF-8 text"),
            Self::NoReleaseConfig => write!(
                f,
                "{RELEASE_CONFIG_PATH} does not exist; there is no release to describe"
            ),
            Self::ReleaseToolchainPath => f.write_str(
                "rust-toolchain.toml names a toolchain path, so rustup would run a cargo from the repository with the release App token in scope; name a channel instead",
            ),
            Self::HelperNotPinned => f.write_str(
                "velnor-ci is not pinned: every generated job runs it, so pin a release with `velnor-gen pin-helper <version>`",
            ),
            Self::ReleaseHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot check the release on the released commit, because it does not recheck the live tag ruleset or the manifests; pin velnor-ci {}.{}.{} or later with `velnor-gen pin-helper {}.{}.{}`",
                RELEASE_HELPER_MINIMUM.0, RELEASE_HELPER_MINIMUM.1, RELEASE_HELPER_MINIMUM.2,
                RELEASE_HELPER_MINIMUM.0, RELEASE_HELPER_MINIMUM.1, RELEASE_HELPER_MINIMUM.2
            ),
            Self::MirrorHelperTooOld(version) => write!(f, "velnor-ci {version} cannot configure crates_mirror; pin velnor-ci 0.1.81 or later"),
            Self::InputsBuildHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot record pre-build directory inventories for inputs audit; pin velnor-ci 0.1.81 or later with `velnor-gen pin-helper 0.1.81`"
            ),
            Self::BudgetHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot check the speed budget; pin velnor-ci 0.1.82 or later with `velnor-gen pin-helper 0.1.82`"
            ),
            Self::PrOnlyChecksHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot enforce pr_only_checks; pin velnor-ci 0.1.82 or later with `velnor-gen pin-helper 0.1.82`"
            ),
            Self::Metadata(error) => error.fmt(f),
            Self::Rules(violations) => {
                f.write_str("the generated workflow breaks the YAML rules:")?;
                for violation in violations {
                    write!(f, "\n  {violation}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for GenerateError {}

impl From<DetectError> for GenerateError {
    fn from(error: DetectError) -> Self {
        Self::Detect(error)
    }
}
