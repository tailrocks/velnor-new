// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod cache;
mod cache_layout;
mod cache_outputs;
mod cron;
mod helper;
mod ids;
mod plan;
mod repo;
mod selection;
mod steps;
mod test_group;
mod verdict;

pub use cache::{CacheKey, CachePath, CacheRoot, Host};
pub use cache_layout::{
    CACHE_COMPAT_LENGTH, CacheKeyLayout, CacheKeySegment, RUNNER_ARCH_MAX_LENGTH,
    RUNNER_OS_MAX_LENGTH, json_char_bytes, json_string_bytes,
};
pub use cache_outputs::{
    BASE_PREFIX_OUTPUT, BASE_SAVE_OUTPUT, CacheLaneOutput, SOURCES_PREFIX_OUTPUT,
    SOURCES_SAVED_OUTPUT, cache_lane_outputs,
};
pub use cron::CronExpr;
pub use helper::{HelperRelease, Platform};
pub use ids::{
    CommitSha, HelperVersion, InvalidId, JobId, PackageName, PathGlob, RepoPath, RootSlug,
    RunnerLabel, Sha256, TargetName, github_hosted,
};
pub use plan::{
    Access, BUILD_TESTS_STEP, CargoCommand, Concurrency, FilterRef, Gate, GateNeed, Job, LINT_STEP,
    PermissionScope, Permissions, PlanFrom, PlanRef, PullRequestType, Shape, ShapeDecision, Step,
    TEST_STEP, TESTS_BY_PACKAGE_OUTPUT, TESTS_LISTED_OUTPUT, TESTS_RUN_OUTPUT, Triggers, When,
    WorkflowPlan, group_output_name,
};
pub use repo::{
    CacheDebug, CiDebugInfo, CompileInputs, CratesMirror, CustomHarness, DependencyKind, FullRun,
    InputRule, MemberDependency, OnMain, PackageDependency, PolicyTools, RepoFacts, RepoView,
    RepositoryIdentity, RustCiConfig, RustMember, RustMetadata, RustPackage, RustRoot, Target,
    TargetKind, TestSetup, Timings, ToolPin, ToolPins, Toolchain, UnownedPackage, Unread,
    Weakening,
};
pub use selection::{
    AllTrigger, ChangeSet, EventKind, OptionalSteps, RELEASE_CONFIG_PATH, RELEASE_PLZ_FILE,
    RELEASE_WORKFLOW_PATH, RustSelection, SelectionReason, SkipReason, StepNeed, WORKFLOW_PATH,
};
pub use test_group::{GroupEvent, GroupRoot, TestGroup, TestNeed};
pub use verdict::{JobCondition, VerdictJob, VerdictSpec};

pub use steps::{BaseExpiry, HelperCommand, StepKind, TrimFamily};
