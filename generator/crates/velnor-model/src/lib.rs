// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod cache;
mod ids;
mod plan;
mod repo;
mod selection;

pub use cache::{CacheKey, CachePath, CacheRoot, Host};
pub use ids::{
    DownloadUrl, HelperVersion, InvalidId, JobId, PackageName, PathGlob, RepoPath, RootSlug,
    RunnerLabel, Sha256, TargetName,
};
pub use plan::{
    Access, CargoCommand, Concurrency, FilterRef, Gate, GateNeed, HelperBinary, HelperCommand,
    HelperRelease, Job, PermissionScope, Permissions, PlanFrom, Platform, Shape, ShapeDecision,
    Step, StepKind, Triggers, When, WorkflowPlan,
};
pub use repo::{
    CiDebugInfo, CompileInputs, DependencyKind, InputRule, MemberDependency, PackageDependency,
    RepoFacts, RepoView, RustCiConfig, RustMember, RustMetadata, RustPackage, RustRoot, Target,
    TargetKind, Timings, ToolPin, ToolPins, Toolchain, UnownedPackage, Unread,
};
pub use selection::{
    AllTrigger, ChangeSet, OptionalSteps, RustSelection, SelectionReason, SkipReason, StepNeed,
    WORKFLOW_PATH,
};
