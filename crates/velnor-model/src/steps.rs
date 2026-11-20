// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache::{CacheKey, CacheRoot};
use crate::helper::HelperRelease;
use crate::ids::{JobId, PackageName, RepoPath, RootSlug};
use crate::lint::LintCommand;
use crate::plan::CargoCommand;
use crate::verdict::VerdictSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputsCli {
    PerRoot,
    Batched,
}

pub const BUDGET_JOB: &str = "rust-budget";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    Checkout {
        depth: u32,
        sparse: Option<Vec<String>>,
    },
    InstallTools {
        only: Option<String>,
    },
    InstallMise,
    Lint(LintCommand),
    CacheRestore(CacheKey),
    CacheSave(CacheKey),
    RestoredTargets {
        expiry: BaseExpiry,
        targets: Vec<RepoPath>,
        cli: InputsCli,
    },
    SourcesLocks(Vec<RepoPath>),
    SourcesFetch {
        lane: RootSlug,
        position: usize,
    },
    SourcesFetchAll {
        lane: RootSlug,
        manifests: Vec<RepoPath>,
    },
    CacheDecide {
        manifests: Vec<RepoPath>,
        target: RepoPath,
        archive: Option<RepoPath>,
        cli: InputsCli,
        budget_bytes: u64,
    },
    CacheResult(Vec<(JobId, TrimFamily)>),
    CacheVerify {
        lane: JobId,
        family: TrimFamily,
    },
    CacheTrim {
        lane: JobId,
        family: TrimFamily,
    },
    DeltaPrepare(CacheRoot),
    DeltaApply(CacheRoot),
    DeltaStage(CacheRoot, Vec<PackageName>),
    CacheBudget(u64),
    RustComponents(Vec<String>),
    RustTargets(Vec<String>),
    UploadArchive {
        name: String,
        file: RepoPath,
    },
    DownloadArchive {
        name: String,
        into: Option<RepoPath>,
    },
    Cargo(CargoCommand),
    Helper(HelperCommand),
    Prepare(PrepareLocks),
    InputsAuditRoot(Option<RepoPath>),
    InputsSave {
        target: RepoPath,
        cli: InputsCli,
    },
    InputsRestore {
        roots: Vec<(RepoPath, RepoPath)>,
        cli: InputsCli,
    },
    TestGroupsCheck(Vec<GroupFilter>),
    CratesMirror(crate::repo::CratesMirror),
    PrivateCheck {
        cached: Vec<crate::ids::RootSlug>,
    },
    PrivateToken {
        sources: crate::repo::PrivateSources,
        cached: Vec<crate::ids::RootSlug>,
    },
    PrivateFetch {
        manifests: Vec<RepoPath>,
        cached: Vec<crate::ids::RootSlug>,
    },
    /// Fetches the `RustSec` advisory DB for each distinct cargo-deny config, so that the
    /// `--offline` deny check finds it. It runs before any private token exists.
    AdvisoryFetch {
        configs: Vec<Option<RepoPath>>,
    },
    MbxRemote,
    MbxStore,
    MbxSeed,
    ToolsLocal(ToolsLocalVerb),
    TestSetup {
        name: String,
        run: String,
    },
    GeneratorCheck,
    InstallHelper(HelperRelease),
    FailWhenANeedFailed,
    FailOnDraftRun,
    DraftNotice,
    ClearRunLabel,
    Verdict {
        spec: VerdictSpec,
        needs_max_bytes: usize,
    },
    UploadCargoLog,
    ReportCaches {
        caches: Vec<CacheKey>,
        nextest_artifacts: bool,
        node_tools: ToolsTelemetry,
    },
    UploadMetrics,
    DownloadMetrics,
    CheckMetrics {
        needs_max_bytes: usize,
        trusted_api: bool,
    },
    Budget {
        last: usize,
    },
    MergeGroupPassed {
        required: String,
    },
    MergeQueueCheck,
}

impl StepKind {
    #[must_use]
    pub const fn is_check(&self) -> bool {
        match self {
            // The advisory DB fetch serves the cargo-deny check, which is a check, so it
            // is skipped after a merge group exactly when that check is.
            Self::TestSetup { .. } | Self::AdvisoryFetch { .. } => true,
            Self::Cargo(command) => !command.is_target_build(),
            Self::Checkout { .. }
            | Self::InstallTools { .. }
            | Self::InstallMise { .. }
            | Self::Lint(_)
            | Self::CacheRestore(_)
            | Self::CacheSave(_)
            | Self::RestoredTargets { .. }
            | Self::SourcesLocks(_)
            | Self::SourcesFetch { .. }
            | Self::SourcesFetchAll { .. }
            | Self::AdvisoryFetch { .. }
            | Self::CacheDecide { .. }
            | Self::CacheResult(_)
            | Self::CacheVerify { .. }
            | Self::CacheTrim { .. }
            | Self::DeltaPrepare(_)
            | Self::DeltaApply(_)
            | Self::DeltaStage(_, _)
            | Self::CacheBudget(_)
            | Self::RustComponents(_)
            | Self::RustTargets(_)
            | Self::UploadArchive { .. }
            | Self::DownloadArchive { .. }
            | Self::Helper(_)
            | Self::Prepare(_)
            | Self::InputsAuditRoot(_)
            | Self::InputsSave { .. }
            | Self::InputsRestore { .. }
            | Self::TestGroupsCheck(_)
            | Self::CratesMirror(_)
            | Self::PrivateCheck { .. }
            | Self::PrivateToken { .. }
            | Self::PrivateFetch { .. }
            | Self::AdvisoryFetch { .. }
            | Self::MbxRemote { .. }
            | Self::MbxStore { .. }
            | Self::MbxSeed { .. }
            | Self::ToolsLocal(_)
            | Self::GeneratorCheck { .. }
            | Self::InstallHelper(_)
            | Self::FailWhenANeedFailed { .. }
            | Self::FailOnDraftRun { .. }
            | Self::DraftNotice { .. }
            | Self::ClearRunLabel { .. }
            | Self::Verdict { .. }
            | Self::UploadCargoLog { .. }
            | Self::ReportCaches { .. }
            | Self::UploadMetrics { .. }
            | Self::DownloadMetrics { .. }
            | Self::CheckMetrics { .. }
            | Self::Budget { .. }
            | Self::MergeGroupPassed { .. }
            | Self::AdvisoryFetch { .. }
            | Self::MergeQueueCheck { .. } => false,
        }
    }

    /// Whether the step saves to the Actions cache service, which `cache-mode: read` denies. The
    /// trim steps delete over the REST API with the job token, which the mode does not gate, and
    /// the node-local stores and the scope-cache directory are not the service.
    #[must_use]
    pub fn saves_to_cache_service(&self) -> bool {
        matches!(self, Self::CacheSave(_) | Self::MbxRemote)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupFilter {
    pub name: String,
    pub filter: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolsLocalVerb {
    Restore,
    Save,
    SaveChecked,
    Prime,
    RecordRestore,
    RecordSave,
}

/// Which tools steps the cache telemetry reads in place of the cache action's own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolsTelemetry {
    pub restore: bool,
    pub save: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimFamily {
    Target,
    Sources,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseExpiry {
    Never,
    OnDefaultBranch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareLocks {
    All,
    Only(Vec<RootSlug>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperCommand {
    Plan,
    PlanFull,
    CheckMetadata,
    Restored(BaseExpiry),
    InputsAudit,
}
