// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::ids::{PackageName, PathGlob, RepoPath, RunnerLabel, TargetName};
use crate::plan::HelperRelease;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoView {
    pub files: BTreeMap<RepoPath, Vec<u8>>,
    pub tracked_paths: BTreeSet<RepoPath>,
    pub rust_metadata: BTreeMap<RepoPath, RustMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RustMetadata {
    pub members: Vec<RustPackage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustPackage {
    pub name: PackageName,
    pub manifest: RepoPath,
    pub dependencies: Vec<PackageDependency>,
    pub targets: Vec<Target>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageDependency {
    pub name: PackageName,
    pub kind: DependencyKind,
    pub manifest: Option<RepoPath>,
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
    pub rust_roots: Vec<RustRoot>,
    pub toolchain: Option<Toolchain>,
    pub tools: ToolPins,
    pub config: RustCiConfig,
    pub helper: Option<HelperRelease>,
    pub compile_inputs: CompileInputs,
    pub unowned: BTreeMap<RepoPath, UnownedPackage>,
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
    pub members: Vec<RustMember>,
    pub depends_on: Vec<RepoPath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustMember {
    pub name: PackageName,
    pub manifest: RepoPath,
    pub directory: Option<RepoPath>,
    pub dependencies: Vec<MemberDependency>,
    pub targets: Vec<Target>,
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
    pub tools: BTreeMap<String, ToolPin>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolPin {
    pub requested: Vec<String>,
    pub locked: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustCiConfig {
    pub runner: RunnerLabel,
    pub timeout_minutes: u16,
    pub exclude: Vec<PathGlob>,
    pub non_inputs: Vec<PathGlob>,
    pub inputs: Vec<InputRule>,
    pub default_ignores: bool,
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
            timeout_minutes: 30,
            exclude: Vec::new(),
            non_inputs: Vec::new(),
            inputs: Vec::new(),
            default_ignores: true,
        }
    }
}
