// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::budget_mode::{ModeBudget, Selection};
use crate::cron::CronExpr;
use crate::hosted::HostedRunners;
use crate::ids::{PackageName, PathGlob, RepoPath, RootSlug, RunnerLabel, TargetName};
use crate::lint::LintJob;
use crate::shape::Shape;
use crate::test_group::TestGroup;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustCiConfig {
    pub runner: RunnerLabel,
    pub fork_runner: RunnerLabel,
    /// Labels declared `hosted = true`; every other runner is self-hosted and fenced.
    pub hosted_runners: HostedRunners,
    pub crates_mirror: Option<CratesMirror>,
    pub private_sources: Option<PrivateSources>,
    pub release_assets: Vec<crate::ReleaseAsset>,
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
    pub test_lanes: Vec<TestLane>,
    pub budget: Option<SpeedBudget>,
    pub lint: Vec<LintJob>,
    pub test_groups: Vec<TestGroup>,
    pub checkout: CheckoutScope,
    pub workspace_groups: Vec<WorkspaceGroup>,
    /// Hand-written workflows under `.github/workflows/` that `velnor-gen check` must not
    /// flag. `None` when the key is not written, so the helper gate can tell an absent key
    /// from an empty list.
    pub external_workflows: Option<Vec<RepoPath>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceGroup {
    pub name: String,
    pub slug: RootSlug,
    pub gate: RootSlug,
    pub roots: Vec<RepoPath>,
}

impl WorkspaceGroup {
    #[must_use]
    pub fn target_dir(&self) -> RepoPath {
        RepoPath::new(format!("target-velnor/{}", self.name))
            .unwrap_or_else(|_| RepoPath::from_static("target"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpeedBudget {
    pub pr_wall_seconds: u64,
    pub main_wall_seconds: u64,
    pub runner_seconds: u64,
    pub setup_seconds: u64,
    /// Limits of runs whose plan selected everything; a key the table leaves
    /// out takes the plain key above.
    pub full: ModeBudget,
    pub affected: ModeBudget,
    pub none: ModeBudget,
}

impl SpeedBudget {
    fn any_mode(&self, check: impl Fn(&ModeBudget) -> bool) -> bool {
        [&self.full, &self.affected, &self.none]
            .into_iter()
            .any(check)
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        let above = |limit: Option<u64>| limit.is_some_and(|seconds| seconds > 0);
        self.pr_wall_seconds > 0
            || self.main_wall_seconds > 0
            || self.runner_seconds > 0
            || self.setup_seconds > 0
            || self.any_mode(|mode| {
                above(mode.pr_wall_seconds)
                    || above(mode.main_wall_seconds)
                    || above(mode.runner_seconds)
                    || above(mode.setup_seconds)
            })
    }

    /// True when some per-mode table sets a limit.
    #[must_use]
    pub fn has_modes(&self) -> bool {
        self.any_mode(ModeBudget::is_set)
    }

    /// True when the pull request wall time is limited for some mode.
    #[must_use]
    pub fn limits_pull_request_wall(&self) -> bool {
        self.pr_wall_seconds > 0
            || self.any_mode(|mode| mode.pr_wall_seconds.is_some_and(|seconds| seconds > 0))
    }

    /// True when the default branch wall time is limited for some mode.
    #[must_use]
    pub fn limits_main_wall(&self) -> bool {
        self.main_wall_seconds > 0
            || self.any_mode(|mode| mode.main_wall_seconds.is_some_and(|seconds| seconds > 0))
    }

    /// The limits of runs of `mode`: each key the mode table leaves out takes
    /// the plain key. A run whose selection is unknown has the plain keys.
    #[must_use]
    pub fn for_mode(&self, mode: Option<Selection>) -> Self {
        let table = match mode {
            Some(Selection::Full) => self.full,
            Some(Selection::Affected) => self.affected,
            Some(Selection::Nothing) => self.none,
            None => ModeBudget::default(),
        };
        Self {
            pr_wall_seconds: table.pr_wall_seconds.unwrap_or(self.pr_wall_seconds),
            main_wall_seconds: table.main_wall_seconds.unwrap_or(self.main_wall_seconds),
            runner_seconds: table.runner_seconds.unwrap_or(self.runner_seconds),
            setup_seconds: table.setup_seconds.unwrap_or(self.setup_seconds),
            ..Self::default()
        }
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

/// The cargo feature selection of a test lane: named features, every feature
/// (`all`), or the named features without the default ones (`no_default`).
/// `all` never comes with names, so a parsed lane is one of these three shapes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct LaneFeatures {
    pub names: Vec<String>,
    pub all: bool,
    pub no_default: bool,
}

impl LaneFeatures {
    #[must_use]
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.all {
            parts.push("all features".to_owned());
        }
        if self.no_default {
            parts.push("no default features".to_owned());
        }
        if !self.names.is_empty() {
            parts.push(format!("features: {}", self.names.join(",")));
        }
        parts.join(", ")
    }

    #[must_use]
    pub fn normalized(&self) -> Self {
        let mut names = self.names.clone();
        names.sort();
        Self {
            names,
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestLane {
    pub root: Option<RepoPath>,
    pub features: LaneFeatures,
    pub filter: Option<String>,
    pub lint: bool,
    pub doc: bool,
    pub packages: Vec<PackageName>,
    pub job: LaneJob,
    pub written: LaneKeysWritten,
}

/// Which optional keys of a `[[test_lane]]` table were written, whatever their values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LaneKeysWritten {
    pub job: bool,
    pub feature_flags: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LaneJob {
    #[default]
    Inline,
    Parallel,
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
            hosted_runners: HostedRunners::new(vec![RunnerLabel::from_static("ubuntu-latest")]),
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
            test_lanes: Vec::new(),
            budget: None,
            lint: Vec::new(),
            test_groups: Vec::new(),
            checkout: CheckoutScope::Full,
            workspace_groups: Vec::new(),
            external_workflows: None,
            release_assets: Vec::new(),
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
