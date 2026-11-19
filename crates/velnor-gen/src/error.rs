// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use velnor_model::{HelperVersion, RELEASE_CONFIG_PATH, RepoPath};
use velnor_release::ReleaseError;
use velnor_render::RuleViolation;
use velnor_rust::{DetectError, MAX_SOURCE_ROOTS, MetadataFileError};
use velnor_workflows::WorkflowsError;

use crate::{COMPATIBLE_HELPER_SERIES, HELPER_MINIMUM};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerateError {
    Detect(DetectError),
    IncompatibleHelper(HelperVersion),
    Release(ReleaseError),
    ReleaseFileNotUtf8(RepoPath),
    NoReleaseConfig,
    ReleaseToolchainPath,
    HelperNotPinned,
    HelperTooOld(HelperVersion),
    MirrorHelperTooOld(HelperVersion),
    PrivateSourcesHelperTooOld(HelperVersion),
    MbxStoreHelperTooOld(HelperVersion),
    PrOnlyChecksHelperTooOld(HelperVersion),
    BudgetHelperTooOld(HelperVersion),
    TestGroupsHelperTooOld(HelperVersion),
    MergeGroupHelperTooOld(HelperVersion),
    PrCiHelperTooOld(HelperVersion),
    DocsHelperTooOld(HelperVersion),
    CheckAloneHelperTooOld(HelperVersion),
    TooManySourceRoots(usize),
    FamilyTagHelperTooOld(HelperVersion),
    TagPushHelperTooOld(HelperVersion),
    TestLaneHelperTooOld(HelperVersion),
    Metadata(MetadataFileError),
    Workflows(WorkflowsError),
    Rules(Vec<RuleViolation>),
    DeltaStagingCollision(RepoPath),
}

impl fmt::Display for GenerateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeltaStagingCollision(path) => write!(f, "tracked path {path} collides with delta cache staging"),
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
            Self::HelperTooOld(version) => write!(
                f,
                "velnor-ci {version} is older than this generator supports; pin velnor-ci {}.{}.{} or later with `velnor-gen pin-helper {}.{}.{}`",
                HELPER_MINIMUM.0, HELPER_MINIMUM.1, HELPER_MINIMUM.2,
                HELPER_MINIMUM.0, HELPER_MINIMUM.1, HELPER_MINIMUM.2
            ),
            Self::MbxStoreHelperTooOld(version) => write!(f, "velnor-ci {version} cannot select the shared MBX store; pin velnor-ci 0.1.85 or later"),
            Self::TooManySourceRoots(roots) => write!(
                f,
                "the repository has {roots} Cargo roots, and the shared Sources cache makes every saving lane fetch every root, which keeps the generated workflow within its size limit only up to {MAX_SOURCE_ROOTS} roots; reduce the Cargo roots to {MAX_SOURCE_ROOTS} or fewer, or pin velnor-ci 0.1.86 or later with `velnor-gen pin-helper 0.1.86`, whose `velnor-ci sources fetch --roots` replaces the per-root steps (tailrocks/velnor-new#1362)"
            ),
            Self::MirrorHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot configure crates_mirror or disable it with VELNOR_CRATES_MIRROR=off; pin velnor-ci 0.1.93 or later with `velnor-gen pin-helper 0.1.93`"
            ),
            Self::PrivateSourcesHelperTooOld(version) => write!(f, "velnor-ci {version} cannot fetch private sources; pin velnor-ci 0.1.86 or later with `velnor-gen pin-helper 0.1.86`"),
            Self::PrCiHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} does not know the pr_ci key of .velnor/rust-ci.toml; pin velnor-ci 0.1.86 or later with `velnor-gen pin-helper 0.1.86`"
            ),
            Self::BudgetHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot check the speed budget; pin velnor-ci 0.1.92 or later with `velnor-gen pin-helper 0.1.92`"
            ),
            Self::PrOnlyChecksHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot enforce pr_only_checks; pin velnor-ci 0.1.82 or later with `velnor-gen pin-helper 0.1.82`"
            ),
            Self::TestGroupsHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot read [[test_groups]] in the configuration; pin velnor-ci 0.1.82 or later with `velnor-gen pin-helper 0.1.82`"
            ),
            Self::MergeGroupHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot tell whether a merge_group run passed, which skip_push_after_merge_group needs; pin velnor-ci 0.1.88 or later with `velnor-gen pin-helper 0.1.88`"
            ),
            Self::DocsHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} does not know the docs key of .velnor/rust-ci.toml; pin velnor-ci 0.1.88 or later with `velnor-gen pin-helper 0.1.88`"
            ),
            Self::CheckAloneHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} does not know the check_alone key of .velnor/rust-ci.toml; pin velnor-ci 0.1.91 or later with `velnor-gen pin-helper 0.1.91`"
            ),
            Self::TestLaneHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} does not know the test_lane key of .velnor/rust-ci.toml; pin velnor-ci 0.1.93 or later with `velnor-gen pin-helper 0.1.93`"
            ),
            Self::FamilyTagHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot bind a family tag; pin velnor-ci 0.1.83 or later with `velnor-gen pin-helper 0.1.83`"
            ),
            Self::TagPushHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot check a pushed family tag, which family_trigger = \"tag_push\" needs; pin velnor-ci 0.1.98 or later with `velnor-gen pin-helper 0.1.98`"
            ),
            Self::Metadata(error) => error.fmt(f),
            Self::Workflows(error) => error.fmt(f),
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
