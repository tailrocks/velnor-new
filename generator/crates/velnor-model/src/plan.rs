// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::cache::CacheKey;
use crate::cron::CronExpr;
use crate::ids::{
    CommitSha, HelperVersion, InvalidId, JobId, PackageName, RepoPath, RootSlug, RunnerLabel,
    Sha256, TargetName,
};
use crate::repo::TargetKind;

pub const LINT_STEP: &str = "Lint";
pub const TEST_STEP: &str = "Test";

pub const TESTS_RUN_OUTPUT: &str = "tests_run";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowPlan {
    pub name: String,
    pub triggers: Triggers,
    pub concurrency: Concurrency,
    pub permissions: Permissions,
    pub jobs: Vec<Job>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Triggers {
    pub pull_request: Vec<PullRequestType>,
    pub push_branch: Option<String>,
    pub merge_group: bool,
    pub schedule: Option<CronExpr>,
    pub workflow_dispatch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestType {
    Opened,
    Synchronize,
    Reopened,
    ReadyForReview,
    Edited,
    ConvertedToDraft,
}

impl PullRequestType {
    pub const ALL: [Self; 5] = [
        Self::Opened,
        Self::Synchronize,
        Self::Reopened,
        Self::ReadyForReview,
        Self::Edited,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Concurrency {
    CancelOlderPullRequestRuns,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Permissions(pub BTreeMap<PermissionScope, Access>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PermissionScope {
    Actions,
    Contents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    None,
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: JobId,
    pub name: Option<String>,
    pub comment: Option<String>,
    pub needs: Vec<JobId>,
    pub when: When,
    pub skip_on_draft: bool,
    pub runs_on: RunnerLabel,
    pub permissions: Permissions,
    pub timeout_minutes: u16,
    pub gate: Option<Gate>,
    pub plan_outputs: Vec<(RootSlug, GateNeed)>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub kind: StepKind,
    pub directory: Option<RepoPath>,
    pub when: When,
    pub skip_on_draft: bool,
    pub gate: Option<Gate>,
    pub reads: Option<PlanRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRef {
    pub slug: RootSlug,
    pub from: PlanFrom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate {
    pub slug: RootSlug,
    pub need: GateNeed,
    pub from: PlanFrom,
    pub package: Option<PackageName>,
}

impl Gate {
    #[must_use]
    pub fn outputs(&self) -> Vec<(RootSlug, GateNeed)> {
        let own = |need| (self.slug.clone(), need);
        match self.need {
            GateNeed::Selected => vec![own(GateNeed::Selected)],
            GateNeed::Doctests | GateNeed::Docs => vec![
                own(GateNeed::Selected),
                own(self.need),
                own(GateNeed::PackageArgs),
            ],
            GateNeed::Deny => vec![own(GateNeed::Deny)],
            GateNeed::Machete => vec![own(GateNeed::Machete)],
            GateNeed::Policy => vec![own(GateNeed::Deny), own(GateNeed::Machete)],
            GateNeed::TestFilter | GateNeed::PackageArgs | GateNeed::DoctestsKnown => {
                vec![own(self.need)]
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GateNeed {
    Selected,
    Doctests,
    DoctestsKnown,
    Docs,
    TestFilter,
    PackageArgs,
    Deny,
    Machete,
    Policy,
}

impl GateNeed {
    #[must_use]
    pub fn is_global(self) -> bool {
        matches!(self, Self::Deny | Self::Machete | Self::Policy)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanFrom {
    ThisJob,
    Job(JobId),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum When {
    #[default]
    Success,
    NotCancelled,
    Scheduled,
    ScheduledOrPush,
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
    GeneratorCheck,
    InstallHelper(HelperRelease),
    HelperFromArtifact,
    PublishHelper {
        binary: RepoPath,
    },
    FailWhenANeedFailed,
    FailOnDraftRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperCommand {
    Plan,
    Prepare,
    CheckMetadata,
    Restored,
    InputsSave,
    InputsRestore,
    InputsAudit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperRelease {
    version: HelperVersion,
    action: CommitSha,
    digests: BTreeMap<Platform, Sha256>,
}

impl HelperRelease {
    pub fn new(
        version: HelperVersion,
        action: CommitSha,
        digests: BTreeMap<Platform, Sha256>,
    ) -> Result<Self, InvalidId> {
        if digests.is_empty() {
            return Err(InvalidId {
                kind: "HelperRelease",
                value: version.to_string(),
            });
        }
        Ok(Self {
            version,
            action,
            digests,
        })
    }

    #[must_use]
    pub fn version(&self) -> &HelperVersion {
        &self.version
    }

    #[must_use]
    pub fn action(&self) -> &CommitSha {
        &self.action
    }

    #[must_use]
    pub fn digests(&self) -> &BTreeMap<Platform, Sha256> {
        &self.digests
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Platform {
    LinuxX64,
    LinuxArm64,
    MacosArm64,
}

impl Platform {
    pub const ALL: [Self; 3] = [Self::LinuxX64, Self::LinuxArm64, Self::MacosArm64];

    #[must_use]
    pub const fn file_suffix(self) -> &'static str {
        match self {
            Self::LinuxX64 => "linux-x64",
            Self::LinuxArm64 => "linux-arm64",
            Self::MacosArm64 => "macos-arm64",
        }
    }

    #[must_use]
    pub const fn runner_key(self) -> &'static str {
        match self {
            Self::LinuxX64 => "Linux-X64",
            Self::LinuxArm64 => "Linux-ARM64",
            Self::MacosArm64 => "macOS-ARM64",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CargoCommand {
    FmtCheck,
    Clippy {
        extra: Vec<String>,
    },
    CheckAlone {
        package: PackageName,
    },
    Build {
        package: PackageName,
    },
    NextestRun {
        filter: FilterRef,
    },
    NextestArchive {
        file: RepoPath,
    },
    NextestRunArchive {
        file: RepoPath,
        filter: FilterRef,
    },
    TestTarget {
        package: PackageName,
        target: TargetName,
        kind: TargetKind,
        features: Vec<String>,
    },
    Doctest,
    Doc,
    Deny {
        manifest: RepoPath,
    },
    Machete,
}

impl CargoCommand {
    #[must_use]
    pub fn plan_values(&self) -> &'static [GateNeed] {
        match self {
            Self::NextestRun { .. } => &[GateNeed::TestFilter, GateNeed::PackageArgs],
            Self::NextestRunArchive { .. } => &[GateNeed::TestFilter],
            Self::Clippy { .. } | Self::Doc | Self::NextestArchive { .. } => {
                &[GateNeed::PackageArgs]
            }
            Self::Doctest => &[GateNeed::PackageArgs, GateNeed::DoctestsKnown],
            _ => &[],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterRef {
    All,
    AllExcept(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    OneJob,
    LintAndTest,
    BuildThenGroups,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeDecision {
    pub shape: Shape,
    pub reason: String,
}
