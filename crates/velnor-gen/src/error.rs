// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use velnor_model::{HelperVersion, RELEASE_CONFIG_PATH, RepoPath};
use velnor_release::ReleaseError;
use velnor_render::RuleViolation;
use velnor_rust::{DetectError, MAX_SOURCE_ROOTS, MetadataFileError};
use velnor_workflows::WorkflowsError;

use crate::helper_gate::{CACHE_DEBUG_HELPER, REQUIRED_CHECK_HELPER};
use crate::{COMPATIBLE_HELPER_SERIES, HELPER_MINIMUM, TEST_LANE_JOB_HELPER};

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
    HostedHelperTooOld(HelperVersion),
    CacheDebugHelperTooOld(HelperVersion),
    RequiredCheckHelperTooOld(HelperVersion),
    InputsLockHelperTooOld(HelperVersion),
    PrivateSourcesHelperTooOld(HelperVersion),
    MbxStoreHelperTooOld(HelperVersion),
    MbxKeyHelperTooOld(HelperVersion),
    PrOnlyChecksHelperTooOld(HelperVersion),
    BudgetHelperTooOld(HelperVersion),
    BudgetModesHelperTooOld(HelperVersion),
    TestGroupsHelperTooOld(HelperVersion),
    MergeGroupHelperTooOld(HelperVersion),
    PrCiHelperTooOld(HelperVersion),
    DocsHelperTooOld(HelperVersion),
    CheckAloneHelperTooOld(HelperVersion),
    TooManySourceRoots(usize),
    FamilyTagHelperTooOld(HelperVersion),
    TagPushHelperTooOld(HelperVersion),
    TestLaneHelperTooOld(HelperVersion),
    TestLaneJobHelperTooOld(HelperVersion),
    HelperLockLacksMacos {
        version: HelperVersion,
        runner: String,
    },
    VersionGroupsHelperTooOld(HelperVersion),
    Metadata(MetadataFileError),
    Workflows(WorkflowsError),
    Rules(Vec<RuleViolation>),
    DeltaStagingCollision(RepoPath),
    ExcludedLockRun(velnor_rust::ExcludedLockFinding),
}

impl fmt::Display for GenerateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeltaStagingCollision(path) => write!(f, "tracked path {path} collides with delta cache staging"),
            Self::Detect(error) => error.fmt(f),
            Self::ExcludedLockRun(finding) => f.write_str(&finding.message()),
            Self::IncompatibleHelper(version) => write!(f, "{}", incompatible_helper(version)),
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
            Self::CacheDebugHelperTooOld(v) => write!(f, "{}", cache_debug_too_old(v)),
            Self::RequiredCheckHelperTooOld(v) => write!(f, "{}", required_check_too_old(v)),
            Self::MbxKeyHelperTooOld(version) => write!(f, "velnor-ci {version} rejects the cache.mbx key; pin velnor-ci 0.1.108 or later with `velnor-gen pin-helper 0.1.108`"),
            Self::MbxStoreHelperTooOld(version) => write!(f, "velnor-ci {version} cannot select the shared MBX store; pin velnor-ci 0.1.85 or later"),
            Self::TooManySourceRoots(roots) => write!(
                f,
                "the repository has {roots} Cargo roots, and the shared Sources cache makes every saving lane fetch every root, which keeps the generated workflow within its size limit only up to {MAX_SOURCE_ROOTS} roots; reduce the Cargo roots to {MAX_SOURCE_ROOTS} or fewer, or pin velnor-ci 0.1.88 or later with `velnor-gen pin-helper 0.1.88`, whose `velnor-ci sources fetch --roots` replaces the per-root steps (tailrocks/velnor-new#1362)"
            ),
            Self::MirrorHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot configure crates_mirror or disable it with VELNOR_CRATES_MIRROR=off; pin velnor-ci 0.1.93 or later with `velnor-gen pin-helper 0.1.93`"
            ),
            Self::HostedHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} does not know the hosted key of .velnor/rust-ci.toml, and reads the file at run time; pin velnor-ci 0.1.99 or later with `velnor-gen pin-helper 0.1.99`"
            ),
            Self::InputsLockHelperTooOld(version) => write!(f, "velnor-ci {version} cannot read the manifest column of .velnor/rust-inputs.lock; pin velnor-ci 0.1.101 or later with `velnor-gen pin-helper 0.1.101`, or write the lock under the older pin"),
            Self::PrivateSourcesHelperTooOld(version) => write!(f, "velnor-ci {version} cannot fetch private sources; pin velnor-ci 0.1.88 or later with `velnor-gen pin-helper 0.1.88`"),
            Self::PrCiHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} does not know the pr_ci key of .velnor/rust-ci.toml; pin velnor-ci 0.1.88 or later with `velnor-gen pin-helper 0.1.88`"
            ),
            Self::BudgetHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot check the speed budget; pin velnor-ci 0.1.92 or later with `velnor-gen pin-helper 0.1.92`"
            ),
            Self::BudgetModesHelperTooOld(v) => write!(f, "{}", modes_too_old(v)),
            Self::PrOnlyChecksHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot enforce pr_only_checks; pin velnor-ci 0.1.82 or later with `velnor-gen pin-helper 0.1.82`"
            ),
            Self::TestGroupsHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot read [[test_groups]] in the configuration; pin velnor-ci 0.1.90 or later with `velnor-gen pin-helper 0.1.90`"
            ),
            Self::MergeGroupHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot tell whether a merge_group run passed, which skip_push_after_merge_group needs; pin velnor-ci 0.1.103 or later with `velnor-gen pin-helper 0.1.103`"
            ),
            Self::DocsHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} does not know the docs key of .velnor/rust-ci.toml; pin velnor-ci 0.1.97 or later with `velnor-gen pin-helper 0.1.97`"
            ),
            Self::CheckAloneHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} does not know the check_alone key of .velnor/rust-ci.toml; pin velnor-ci 0.1.97 or later with `velnor-gen pin-helper 0.1.97`"
            ),
            Self::TestLaneHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} does not know the test_lane key of .velnor/rust-ci.toml; pin velnor-ci 0.1.93 or later with `velnor-gen pin-helper 0.1.93`"
            ),
            Self::TestLaneJobHelperTooOld(version) => f.write_str(&lane_job_too_old(version)),
            Self::HelperLockLacksMacos { version, runner } => write!(
                f,
                "the runner `{runner}` is a macOS runner, but the velnor-ci {version} in .velnor/rust-ci.lock has no macos-arm64 digest, so `actions/setup-velnor` would fail before velnor-ci runs; pin a release that lists a macos-arm64 digest with `velnor-gen pin-helper <version>`, or use a Linux runner"
            ),
            Self::FamilyTagHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot bind a family tag; pin velnor-ci 0.1.85 or later with `velnor-gen pin-helper 0.1.85`"
            ),
            Self::TagPushHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot check a pushed family tag, which family_trigger = \"tag_push\" needs; pin velnor-ci 0.1.98 or later with `velnor-gen pin-helper 0.1.98`"
            ),
            Self::VersionGroupsHelperTooOld(version) => write!(
                f,
                "velnor-ci {version} cannot record the members of a release-plz version_group as one release; pin velnor-ci 0.1.97 or later with `velnor-gen pin-helper 0.1.97`"
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

fn incompatible_helper(version: &HelperVersion) -> String {
    format!(
        "velnor-ci {version} does not fit this velnor-gen, which works with velnor-ci {}.{}.x; pin a compatible release with `velnor-gen pin-helper <version>`",
        COMPATIBLE_HELPER_SERIES.0, COMPATIBLE_HELPER_SERIES.1
    )
}

fn cache_debug_too_old(version: &HelperVersion) -> String {
    let (major, minor, patch) = CACHE_DEBUG_HELPER;
    format!(
        "velnor-ci {version} does not know the cache_debug key of .velnor/rust-ci.toml; pin velnor-ci {major}.{minor}.{patch} or later with `velnor-gen pin-helper {major}.{minor}.{patch}`"
    )
}

fn required_check_too_old(version: &HelperVersion) -> String {
    let (major, minor, patch) = REQUIRED_CHECK_HELPER;
    format!(
        "velnor-ci {version} does not name a required_check other than Required for a merge queue run; pin velnor-ci {major}.{minor}.{patch} or later with `velnor-gen pin-helper {major}.{minor}.{patch}`"
    )
}

fn lane_job_too_old(version: &HelperVersion) -> String {
    let (major, minor, patch) = TEST_LANE_JOB_HELPER;
    format!(
        "velnor-ci {version} does not know the job key of a [[test_lane]] in .velnor/rust-ci.toml; pin velnor-ci {major}.{minor}.{patch} or later with `velnor-gen pin-helper {major}.{minor}.{patch}`"
    )
}

fn modes_too_old(version: &HelperVersion) -> String {
    format!(
        "velnor-ci {version} cannot read the selection of a run, so it cannot apply [budget.full], [budget.affected] or [budget.none]; pin velnor-ci 0.1.104 or later with `velnor-gen pin-helper 0.1.104`"
    )
}
