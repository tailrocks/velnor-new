// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cron::CronExpr;
use crate::ids::{PackageName, PathGlob, RepoPath, RunnerLabel, TargetName};
use crate::lint::LintJob;
use crate::plan::Shape;
use crate::test_group::TestGroup;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustCiConfig {
    pub runner: RunnerLabel,
    pub fork_runner: RunnerLabel,
    pub crates_mirror: Option<CratesMirror>,
    pub private_sources: Option<PrivateSources>,
    pub mbx_store: MbxStore,
    pub docs_run: DocsRun,
    pub check_alone_run: DocsRun,
    pub timeout_minutes: u16,
    pub exclude: Vec<PathGlob>,
    pub non_inputs: Vec<PathGlob>,
    pub inputs: Vec<InputRule>,
    pub default_ignores: bool,
    pub shape: Option<Shape>,
    pub required_check: String,
    pub nextest_harnesses: Vec<(PackageName, TargetName)>,
    pub default_branch: String,
    pub skip_draft_pull_requests: bool,
    pub pr_ci: PrCi,
    pub on_main: OnMain,
    pub full_run: FullRun,
    pub helper: Option<RepoPath>,
    pub ci_debuginfo: CiDebugInfo,
    pub cache_budget_bytes: u64,
    pub cache_check_blocks: bool,
    pub cache_debug: CacheDebug,
    pub mbx_remote: MbxRemote,
    pub push_after_merge_group: PushAfterMergeGroup,
    pub test_setup: Vec<TestSetup>,
    pub budget: Option<SpeedBudget>,
    pub lint: Vec<LintJob>,
    pub test_groups: Vec<TestGroup>,
    pub checkout: CheckoutScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpeedBudget {
    pub pr_wall_seconds: u64,
    pub main_wall_seconds: u64,
    pub runner_seconds: u64,
    pub setup_seconds: u64,
}

impl SpeedBudget {
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.pr_wall_seconds > 0
            || self.main_wall_seconds > 0
            || self.runner_seconds > 0
            || self.setup_seconds > 0
    }

    #[must_use]
    pub fn wall_seconds(&self, pull_request: bool) -> u64 {
        if pull_request {
            self.pr_wall_seconds
        } else {
            self.main_wall_seconds
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestSetup {
    pub root: Option<RepoPath>,
    pub name: String,
    pub run: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnMain {
    Affected,
    Everything,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FullRun {
    Off,
    Hourly,
    Nightly,
    Weekly,
    Cron(CronExpr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MbxRemote {
    Off,
    GithubActions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushAfterMergeGroup {
    Run,
    SkipWhenQueueRunPassed,
}

impl From<bool> for PushAfterMergeGroup {
    fn from(skip: bool) -> Self {
        if skip {
            Self::SkipWhenQueueRunPassed
        } else {
            Self::Run
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrCi {
    Always,
    Label,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheDebug {
    Off,
    On,
}

impl From<bool> for CacheDebug {
    fn from(on: bool) -> Self {
        if on { Self::On } else { Self::Off }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocsRun {
    Always,
    PostMerge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MbxStore {
    Private,
    Shared,
}

impl From<bool> for MbxStore {
    fn from(shared: bool) -> Self {
        if shared { Self::Shared } else { Self::Private }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiDebugInfo {
    Full,
    LineTablesOnly,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputRule {
    pub glob: PathGlob,
    pub packages: Vec<PackageName>,
}

impl Default for RustCiConfig {
    fn default() -> Self {
        Self {
            runner: RunnerLabel::from_static("ubuntu-latest"),
            fork_runner: RunnerLabel::from_static("ubuntu-latest"),
            crates_mirror: None,
            private_sources: None,
            mbx_store: MbxStore::Private,
            docs_run: DocsRun::Always,
            check_alone_run: DocsRun::Always,
            timeout_minutes: 30,
            exclude: Vec::new(),
            non_inputs: Vec::new(),
            inputs: Vec::new(),
            default_ignores: true,
            shape: None,
            required_check: "Required".to_owned(),
            nextest_harnesses: Vec::new(),
            default_branch: "main".to_owned(),
            skip_draft_pull_requests: false,
            pr_ci: PrCi::Always,
            on_main: OnMain::Affected,
            full_run: FullRun::Nightly,
            helper: None,
            ci_debuginfo: CiDebugInfo::LineTablesOnly,
            cache_budget_bytes: 8_000_000_000,
            cache_check_blocks: false,
            cache_debug: CacheDebug::Off,
            mbx_remote: MbxRemote::Off,
            push_after_merge_group: PushAfterMergeGroup::Run,
            test_setup: Vec::new(),
            budget: None,
            lint: Vec::new(),
            test_groups: Vec::new(),
            checkout: CheckoutScope::Full,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CratesMirror {
    pub runner: RunnerLabel,
    pub registry: String,
    pub cainfo: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateSources {
    pub app_id: String,
    pub private_key: String,
    pub owner: String,
    pub repositories: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckoutScope {
    Full,
    Sparse,
}
