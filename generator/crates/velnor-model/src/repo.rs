// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::cron::CronExpr;
use crate::helper::HelperRelease;
use crate::ids::{PackageName, PathGlob, RepoPath, RootSlug, RunnerLabel, TargetName};
use crate::plan::Shape;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoView {
    pub repository: RepositoryIdentity,
    pub files: BTreeMap<RepoPath, Vec<u8>>,
    pub tracked_paths: BTreeSet<RepoPath>,
    pub symlinks: BTreeSet<RepoPath>,
    pub rust_metadata: BTreeMap<RepoPath, RustMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryIdentity {
    Named(String),
    Unhosted,
}

impl Default for RepositoryIdentity {
    fn default() -> Self {
        Self::Named(String::new())
    }
}

impl FullRun {
    #[must_use]
    pub fn is_hashed(&self) -> bool {
        matches!(self, Self::Hourly | Self::Nightly | Self::Weekly)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RustMetadata {
    pub members: Vec<RustPackage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustPackage {
    pub name: PackageName,
    pub version: Option<String>,
    pub manifest: RepoPath,
    pub dependencies: Vec<PackageDependency>,
    pub targets: Vec<Target>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageDependency {
    pub name: PackageName,
    pub kind: DependencyKind,
    pub platform: Option<String>,
    pub manifest: Option<RepoPath>,
    pub features: Vec<String>,
    pub rename: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    Normal,
    Dev,
    Build,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub name: TargetName,
    pub kinds: Vec<TargetKind>,
    pub tests: bool,
    pub doctest: bool,
    pub source: Option<RepoPath>,
    pub requires_features: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Lib,
    ProcMacro,
    Bin,
    Test,
    Bench,
    Example,
    BuildScript,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFacts {
    pub repository: String,
    pub rust_roots: Vec<RustRoot>,
    pub toolchain: Option<Toolchain>,
    pub tools: ToolPins,
    pub config: RustCiConfig,
    pub helper: Option<HelperRelease>,
    pub compile_inputs: CompileInputs,
    pub timings: BTreeMap<RepoPath, Timings>,
    pub timings_notes: Vec<String>,
    pub private_sources: Option<String>,
    pub unowned: BTreeMap<RepoPath, UnownedPackage>,
    pub policy: PolicyTools,
    pub weakenings: Vec<Weakening>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PolicyTools {
    pub deny: bool,
    pub machete: bool,
}

impl PolicyTools {
    #[must_use]
    pub fn any(self) -> bool {
        self.deny || self.machete
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Weakening {
    pub file: RepoPath,
    pub setting: String,
    pub effect: String,
}

impl fmt::Display for Weakening {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.file, self.setting, self.effect)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Timings {
    pub lint_build_seconds: u64,
    pub test_build_seconds: u64,
    pub test_seconds: u64,
    pub test_binary_seconds: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnownedPackage {
    pub name: PackageName,
    pub dependencies: Vec<MemberDependency>,
    pub unread: Option<Unread>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unread {
    Missing,
    Invalid(String),
    Git(String),
    Inherited(String),
    Outside(String),
}

impl fmt::Display for Unread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("its manifest is not in the checkout"),
            Self::Invalid(detail) => write!(f, "its manifest does not parse: {detail}"),
            Self::Git(key) => write!(f, "it has the git dependency {key}"),
            Self::Inherited(key) => {
                write!(
                    f,
                    "its workspace does not say where the dependency {key} is"
                )
            }
            Self::Outside(key) => write!(f, "its dependency {key} is outside the repository"),
        }
    }
}

pub type CompileInputs = BTreeMap<Option<RepoPath>, BTreeSet<PackageName>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustRoot {
    pub manifest: RepoPath,
    pub target_dir: RepoPath,
    pub nextest_ci_profile: bool,
    pub members: Vec<RustMember>,
    pub depends_on: Vec<RepoPath>,
    pub external_dependencies: Vec<PackageName>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustMember {
    pub name: PackageName,
    pub manifest: RepoPath,
    pub directory: Option<RepoPath>,
    pub dependencies: Vec<MemberDependency>,
    pub targets: Vec<Target>,
    pub custom_harnesses: Vec<CustomHarness>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomHarness {
    pub name: TargetName,
    pub kind: TargetKind,
    pub required_features: Vec<String>,
}

impl RustMember {
    #[must_use]
    pub fn has_test_target(&self) -> bool {
        self.targets.iter().any(|target| target.tests)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberDependency {
    pub name: PackageName,
    pub kind: DependencyKind,
    pub platform: Option<String>,
    pub manifest: RepoPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toolchain {
    pub channel: Option<String>,
    pub path: Option<String>,
    pub components: Vec<String>,
    pub targets: Vec<String>,
    pub profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPins {
    pub configs: Vec<RepoPath>,
    pub locked_file: bool,
    pub tools: BTreeMap<String, ToolPin>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolPin {
    pub requested: Vec<String>,
    pub locked: Vec<String>,
    pub os_filters: BTreeMap<String, Vec<String>>,
    pub prebuilt: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustCiConfig {
    pub runner: RunnerLabel,
    pub fork_runner: RunnerLabel,
    pub crates_mirror: Option<CratesMirror>,
    pub mbx_store: MbxStore,
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
    pub on_main: OnMain,
    pub full_run: FullRun,
    pub helper: Option<RepoPath>,
    pub ci_debuginfo: CiDebugInfo,
    pub cache_budget_bytes: u64,
    pub cache_check_blocks: bool,
    pub cache_debug: CacheDebug,
    pub test_setup: Vec<TestSetup>,
    pub workspace_groups: Vec<WorkspaceGroup>,
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
            mbx_store: MbxStore::Private,
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
            on_main: OnMain::Affected,
            full_run: FullRun::Nightly,
            helper: None,
            ci_debuginfo: CiDebugInfo::LineTablesOnly,
            cache_budget_bytes: 8_000_000_000,
            cache_check_blocks: false,
            cache_debug: CacheDebug::Off,
            test_setup: Vec::new(),
            workspace_groups: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CratesMirror {
    pub runner: RunnerLabel,
    pub registry: String,
    pub cainfo: String,
}
