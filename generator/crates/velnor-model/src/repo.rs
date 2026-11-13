// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::ids::{PackageName, RepoPath, RunnerLabel, TargetName};

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
    pub path: Option<RepoPath>,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Lib,
    Bin,
    Test,
    Bench,
    Example,
    BuildScript,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFacts {
    pub rust_roots: Vec<RustRoot>,
    pub tools: ToolPins,
    pub config: RustCiConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustRoot {
    pub manifest: RepoPath,
    pub packages: Vec<PackageName>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolPins {
    pub mise: Option<RepoPath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustCiConfig {
    pub runner: RunnerLabel,
    pub timeout_minutes: u16,
    pub exclude: Vec<RepoPath>,
}

impl Default for RustCiConfig {
    fn default() -> Self {
        Self {
            runner: RunnerLabel::from_static("ubuntu-latest"),
            timeout_minutes: 30,
            exclude: Vec::new(),
        }
    }
}
