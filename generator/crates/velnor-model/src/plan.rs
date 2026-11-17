// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::cron::CronExpr;
use crate::ids::{JobId, PackageName, RepoPath, RootSlug, RunnerLabel, TargetName};
use crate::repo::TargetKind;
use crate::steps::StepKind;

pub const LINT_STEP: &str = "Lint";
pub const TEST_STEP: &str = "Test";
pub const BUILD_TESTS_STEP: &str = "Build the tests";
pub const TESTS_RUN_OUTPUT: &str = "tests_run";
pub const TESTS_LISTED_OUTPUT: &str = "tests_listed";
pub const TESTS_BY_PACKAGE_OUTPUT: &str = "tests_by_package";

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
    PullRequests,
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
            GateNeed::TestFilter
            | GateNeed::PackageArgs
            | GateNeed::DoctestsKnown
            | GateNeed::TestsExpected => {
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
    TestsExpected,
}

impl GateNeed {
    #[must_use]
    pub fn is_global(self) -> bool {
        matches!(self, Self::Deny | Self::Machete | Self::Policy)
    }

    #[must_use]
    pub fn output_name(self, slug: &RootSlug) -> String {
        let name = match self {
            Self::Selected => "mode",
            Self::Doctests => "doctests",
            Self::DoctestsKnown => "doctests_known",
            Self::Docs => "docs",
            Self::TestFilter => "test_filter",
            Self::PackageArgs => "package_args",
            Self::Deny => "deny",
            Self::Machete => "machete",
            Self::Policy => "policy",
            Self::TestsExpected => "tests",
        };
        if self.is_global() {
            format!("rust_{name}")
        } else {
            format!("rust_{slug}_{name}")
        }
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
    Finalizer,
    Always,
    Scheduled,
    ScheduledOrPush,
    CacheDebug,
    SaveToolsOnDefaultBranch,
    SaveBase,
    SaveBaseDecided,
    SaveSources,
    FullAudit,
    FullAuditOrPush,
    BaseExpired,
    SavingRun,
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
    BuildBinary {
        package: PackageName,
        binary: TargetName,
    },
    BuildAll,
    NextestBuild,
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
            Self::NextestRun { .. } | Self::NextestRunArchive { .. } => &[GateNeed::TestFilter],
            Self::Doctest => &[GateNeed::DoctestsKnown],
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
