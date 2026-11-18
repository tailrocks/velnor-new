// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache::CacheKey;
use crate::helper::HelperRelease;
use crate::ids::{JobId, RepoPath};
use crate::plan::CargoCommand;
use crate::verdict::VerdictSpec;

/// The `velnor-ci` command line of the input and base-cache steps. A helper
/// release renders only the commands it has: 0.1.81 has the per-root ones,
/// and the batched ones need 0.1.82.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputsCli {
    PerRoot,
    Batched,
}

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
        cli: InputsCli,
    },
    CacheDecide {
        manifests: Vec<RepoPath>,
        target: RepoPath,
        archive: Option<RepoPath>,
        cli: InputsCli,
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
    CratesMirror(crate::repo::CratesMirror),
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
