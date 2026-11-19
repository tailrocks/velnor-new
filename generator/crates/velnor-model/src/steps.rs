// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache::CacheKey;
use crate::helper::HelperRelease;
use crate::ids::{JobId, RepoPath, RootSlug};
use crate::plan::CargoCommand;
use crate::verdict::VerdictSpec;

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
    },
    InstallTools {
        only: Option<String>,
    },
    CacheRestore(CacheKey),
    CacheSave(CacheKey),
    RestoredTargets {
        expiry: BaseExpiry,
        targets: Vec<RepoPath>,
    },
    SourcesLocks(Vec<RepoPath>),
    SourcesFetch {
        lane: RootSlug,
        position: usize,
    },
    CacheDecide {
        manifests: Vec<RepoPath>,
        target: RepoPath,
        archive: Option<RepoPath>,
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
    InputsSave(RepoPath),
    InputsRestore(RepoPath),
    CratesMirror(crate::repo::CratesMirror),
    MbxStore,
    MbxSeed,
    TestSetup {
        name: String,
        run: String,
    },
    GeneratorCheck,
    InstallHelper(HelperRelease),
    FailWhenANeedFailed,
    FailOnDraftRun,
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
