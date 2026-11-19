// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::helper::HelperRelease;
use crate::ids::{PackageName, RepoPath, TargetName};

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
    pub optional: bool,
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
    pub private_suggestion: Option<String>,
    pub unowned: BTreeMap<RepoPath, UnownedPackage>,
    pub policy: PolicyTools,
    pub deny_configs: BTreeMap<RepoPath, RepoPath>,
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
    pub optional: bool,
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

pub use crate::repo_config::{
    CacheDebug, CheckoutScope, CiDebugInfo, CratesMirror, DocsRun, FullRun, InputRule, MbxRemote,
    MbxStore, OnMain, PrCi, PrivateSources, PushAfterMergeGroup, RustCiConfig, SpeedBudget,
    TestSetup,
};
