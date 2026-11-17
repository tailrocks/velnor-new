// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache::{CacheKey, CacheRoot};
use crate::ids::{JobId, PackageName, RepoPath};
use crate::plan::{CargoCommand, HelperRelease};
use crate::verdict::VerdictSpec;

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
    CacheDecide {
        target: RepoPath,
        archive: Option<RepoPath>,
    },
    CacheTrim {
        lane: JobId,
        family: TrimFamily,
    },
    DeltaPrepare(CacheRoot),
    DeltaApply(CacheRoot),
    DeltaStage(CacheRoot, Vec<PackageName>),
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
    GeneratorCheck,
    InstallHelper(HelperRelease),
    HelperFromArtifact,
    PublishHelper {
        binary: RepoPath,
    },
    FailWhenANeedFailed,
    FailOnDraftRun,
    Verdict {
        spec: VerdictSpec,
        needs_max_bytes: usize,
    },
    UploadCargoLog,
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
