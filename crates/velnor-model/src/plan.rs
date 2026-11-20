// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::budget_mode::SelectionName;
use crate::cron::CronExpr;
use crate::hosted::HostedRunners;
use crate::ids::{JobId, PackageName, RepoPath, RootSlug, RunnerLabel, TargetName};
use crate::repo::TargetKind;
use crate::steps::StepKind;

pub const LINT_STEP: &str = "Lint";
pub const TEST_STEP: &str = "Test";
pub const BUILD_TESTS_STEP: &str = "Build the tests";
pub const TESTS_RUN_OUTPUT: &str = "tests_run";
/// Whether the test step built the test binaries: `true` once nextest got past the build.
pub const BUILT_OUTPUT: &str = "built";
pub const TESTS_LISTED_OUTPUT: &str = "tests_listed";
pub const TESTS_IGNORED_OUTPUT: &str = "tests_ignored";
pub const TESTS_BY_PACKAGE_OUTPUT: &str = "tests_by_package";
pub const MERGE_GROUP_STEP: &str = "merge_group";
pub const MERGE_GROUP_SKIP_OUTPUT: &str = "skip_push";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowPlan {
    pub name: String,
    pub triggers: Triggers,
    pub concurrency: Concurrency,
    pub permissions: Permissions,
    pub fork_runner: RunnerLabel,
    pub hosted_runners: HostedRunners,
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
    Labeled,
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
    CancelOlderPullRequestRunsExceptUnrelatedLabels,
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
    pub pr: PrRun,
    pub runs_on: RunnerLabel,
    pub permissions: Permissions,
    pub timeout_minutes: u16,
    pub gate: Option<Gate>,
    pub skip_after_merge_group: Option<PlanFrom>,
    pub plan_outputs: Vec<(RootSlug, GateNeed)>,
    pub group_outputs: Vec<(RootSlug, JobId)>,
    pub reports_ignored: bool,
    /// The pinned helper records the build in its test step (`outputs.built`), so the saves
    /// may wait for that record instead of a build step.
    pub records_build: bool,
    /// Titles the job with the selection of the run, read from the plan outputs.
    pub selection_name: Option<SelectionName>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub kind: StepKind,
    pub directory: Option<RepoPath>,
    pub manifest: Option<RepoPath>,
    pub target_dir: Option<RepoPath>,
    pub when: When,
    pub skip_after_merge_group: Option<PlanFrom>,
    pub skip_when_cold: bool,
    pub pr: PrRun,
    pub gate: Option<Gate>,
    pub reads: Option<PlanRef>,
}

pub const CI_RUN_LABEL: &str = "ci:run";
pub const CI_AUTO_LABEL: &str = "ci:auto";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PrRun {
    #[default]
    Always,
    Ready,
    Quick,
    Related,
    Requested,
    Unrequested,
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
    /// A gate on packages passes when the plan selects any of them or the
    /// whole workspace. Empty means no package condition.
    pub packages: Vec<PackageName>,
    /// Passes exactly when the plan selects a package of the root but none
    /// of `packages`, so a notice can say why the guarded step was skipped.
    pub unselected: bool,
    pub group: Option<JobId>,
}

impl Gate {
    #[must_use]
    pub fn outputs(&self) -> Vec<(RootSlug, GateNeed)> {
        let own = |need| (self.slug.clone(), need);
        match self.need {
            GateNeed::Selected | GateNeed::Group => vec![own(GateNeed::Selected)],
            GateNeed::Doctests | GateNeed::Docs => vec![
                own(GateNeed::Selected),
                own(self.need),
                own(GateNeed::PackageArgs),
            ],
            GateNeed::Deny => vec![own(GateNeed::Deny)],
            GateNeed::Machete => vec![own(GateNeed::Machete)],
            GateNeed::PackageArgs if self.unselected => {
                vec![own(GateNeed::Selected), own(GateNeed::PackageArgs)]
            }
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
    Group,
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
            Self::Group => "group",
        };
        if self.is_global() {
            format!("rust_{name}")
        } else {
            format!("rust_{slug}_{name}")
        }
    }
}

#[must_use]
pub fn group_output_name(slug: &RootSlug, group: &JobId) -> String {
    format!("rust_{slug}_group_{group}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanFrom {
    ThisJob,
    Job(JobId),
    /// The plan outputs of a job that also builds and tests, so a job that
    /// needs it must not wait for that job to succeed.
    WorkJob(JobId),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum When {
    #[default]
    Success,
    NotCancelled,
    NotCancelledPostMerge,
    Finalizer,
    Always,
    Scheduled,
    ScheduledOrPush,
    DefaultBranchPush,
    /// A job that runs on a push to the default branch once the jobs it needs
    /// succeeded, also when an ancestor of those jobs was skipped, and never in
    /// a cancelled run.
    DefaultBranchPushAfterNeeds,
    /// Like `DefaultBranchPushAfterNeeds`, and on the schedule.
    ScheduledOrDefaultBranchPushAfterNeeds,
    CacheDebug,
    SaveToolsOnDefaultBranch,
    RestoreToolsUnlessLocal,
    SaveToolsUnlessLocal,
    PrimeToolsFromNetwork,
    SaveBase,
    SaveBaseDecided,
    SaveSources,
    FetchSources,
    FullAudit,
    FullAuditOrPush,
    PullRequest,
    SaveDelta,
    SaveDeltaDecided,
    BaseExpired,
    SavingRun,
    ClearRunLabel,
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
        ci_profile: bool,
    },
    NextestFeatures {
        features: Vec<String>,
        packages: Vec<PackageName>,
        filter: FilterRef,
        ci_profile: bool,
        reports: bool,
    },
    ClippyFeatures {
        features: Vec<String>,
    },
    DocFeatures {
        features: Vec<String>,
    },
    LaneGuard {
        features: Vec<String>,
        packages: Vec<PackageName>,
        filter: String,
        except: Vec<String>,
    },
    NextestArchive {
        file: RepoPath,
    },
    NextestRunArchive {
        file: RepoPath,
        filter: FilterRef,
        threads: Option<u16>,
        ci_profile: bool,
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
        config: Option<RepoPath>,
    },
    Machete,
}

impl CargoCommand {
    #[must_use]
    pub fn builds(&self) -> bool {
        !matches!(
            self,
            Self::FmtCheck | Self::Deny { .. } | Self::Machete | Self::NextestRunArchive { .. }
        )
    }

    #[must_use]
    pub const fn reports_tests(&self) -> bool {
        matches!(
            self,
            Self::NextestRun { .. }
                | Self::NextestRunArchive { .. }
                | Self::NextestFeatures { reports: true, .. }
        )
    }

    #[must_use]
    pub fn plan_values(&self) -> &'static [GateNeed] {
        match self {
            Self::NextestRun { .. }
            | Self::NextestFeatures { .. }
            | Self::NextestRunArchive { .. } => &[GateNeed::TestFilter],
            Self::Doctest => &[GateNeed::DoctestsKnown],
            _ => &[],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterRef {
    All,
    AllExcept(Vec<String>),
    Expr {
        expr: String,
        except: Vec<String>,
    },
    Group {
        include: Option<String>,
        exclude: Vec<String>,
        except: Vec<String>,
    },
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
