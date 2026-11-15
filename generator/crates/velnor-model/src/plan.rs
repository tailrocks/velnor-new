// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::ids::{
    DownloadUrl, HelperVersion, InvalidId, JobId, PackageName, RepoPath, RunnerLabel, Sha256,
};

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
    pub workflow_dispatch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestType {
    Opened,
    Edited,
    Synchronize,
    Reopened,
    ReadyForReview,
}

impl PullRequestType {
    pub const ALL: [Self; 5] = [
        Self::Opened,
        Self::Edited,
        Self::Synchronize,
        Self::Reopened,
        Self::ReadyForReview,
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
    pub plan_outputs: Vec<(String, GateNeed)>,
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
    pub slug: String,
    pub from: PlanFrom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate {
    pub slug: String,
    pub need: GateNeed,
    pub from: PlanFrom,
}

impl Gate {
    #[must_use]
    pub fn outputs(&self) -> Vec<(String, GateNeed)> {
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
            GateNeed::TestFilter | GateNeed::PackageArgs => vec![own(self.need)],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GateNeed {
    Selected,
    Doctests,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    Checkout { depth: u32 },
    InstallTools,
    RustComponents(Vec<String>),
    RustTargets(Vec<String>),
    Cargo(CargoCommand),
    Helper(HelperCommand),
    GeneratorCheck,
    FetchHelper(HelperRelease),
    FailWhenANeedFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperCommand {
    Plan,
    Prepare,
    Restored,
    InputsSave,
    InputsRestore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperRelease {
    version: HelperVersion,
    binaries: BTreeMap<Platform, HelperBinary>,
}

impl HelperRelease {
    pub fn new(
        version: HelperVersion,
        binaries: BTreeMap<Platform, HelperBinary>,
    ) -> Result<Self, InvalidId> {
        if binaries.is_empty() {
            return Err(InvalidId {
                kind: "HelperRelease",
                value: version.to_string(),
            });
        }
        Ok(Self { version, binaries })
    }

    #[must_use]
    pub fn version(&self) -> &HelperVersion {
        &self.version
    }

    #[must_use]
    pub fn binaries(&self) -> &BTreeMap<Platform, HelperBinary> {
        &self.binaries
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperBinary {
    pub url: DownloadUrl,
    pub sha256: Sha256,
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
    Clippy { extra: Vec<String> },
    CheckAlone { package: PackageName },
    NextestRun,
    NextestArchive { file: RepoPath },
    Doctest,
    Doc,
    Deny { manifest: RepoPath },
    Machete,
}

impl CargoCommand {
    #[must_use]
    pub fn plan_values(&self) -> &'static [GateNeed] {
        match self {
            Self::NextestRun => &[GateNeed::TestFilter, GateNeed::PackageArgs],
            Self::Clippy { .. } => &[GateNeed::PackageArgs],
            Self::Doc | Self::Doctest => &[GateNeed::PackageArgs],
            _ => &[],
        }
    }
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
