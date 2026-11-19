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

pub const BUDGET_STEP: &str = "budget";
pub const BUDGET_JOB: &str = "rust-budget";
pub const BUDGET_PR_WALL: &str = "budget_pr_wall_seconds";
pub const BUDGET_MAIN_WALL: &str = "budget_main_wall_seconds";
pub const BUDGET_RUNNER: &str = "budget_runner_seconds";
pub const BUDGET_SETUP: &str = "budget_setup_seconds";
pub const RUN_WALL: &str = "run_wall_seconds";
pub const RUN_RUNNER: &str = "run_runner_seconds";
pub const BUDGET_OUTPUTS: [&str; 6] = [
    BUDGET_PR_WALL,
    BUDGET_MAIN_WALL,
    BUDGET_RUNNER,
    BUDGET_SETUP,
    RUN_WALL,
    RUN_RUNNER,
];

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperCommand {
    Plan,
    PlanFull,
    Prepare,
    CheckMetadata,
    Restored(BaseExpiry),
    InputsAudit,
}
