// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::ids::{PackageName, RepoPath};

pub const WORKFLOW_PATH: RepoPath = RepoPath::from_static(".github/workflows/rust-ci.yml");

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeSet {
    pub paths: BTreeSet<RepoPath>,
    pub base_files: BTreeMap<RepoPath, String>,
    pub head_files: BTreeMap<RepoPath, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RustSelection {
    None,
    Some(BTreeMap<PackageName, SelectionReason>),
    All(AllTrigger),
}

impl RustSelection {
    #[must_use]
    pub fn of(packages: BTreeMap<PackageName, SelectionReason>, members: usize) -> Self {
        if packages.is_empty() {
            Self::None
        } else if packages.len() * 2 > members {
            Self::All(AllTrigger::MostPackages(packages))
        } else {
            Self::Some(packages)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionReason {
    Changed(RepoPath),
    DependsOn(PackageName),
    DependsOnUnread(PackageName),
    Input(RepoPath),
    Lock(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllTrigger {
    Event,
    ToolchainFile(RepoPath),
    CargoConfig(RepoPath),
    WorkspaceManifest,
    UnknownPath(RepoPath),
    Global(RepoPath),
    GeneratedWorkflow,
    BaseUnavailable,
    MostPackages(BTreeMap<PackageName, SelectionReason>),
}

impl fmt::Display for SelectionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changed(path) => write!(f, "changed:{path}"),
            Self::DependsOn(package) => write!(f, "depends-on:{package}"),
            Self::DependsOnUnread(package) => write!(f, "depends-on-unread:{package}"),
            Self::Input(path) => write!(f, "input:{path}"),
            Self::Lock(dependency) => write!(f, "lock:{dependency}"),
        }
    }
}

impl fmt::Display for AllTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Event => f.write_str("all:event"),
            Self::ToolchainFile(path) => write!(f, "all:toolchain-file:{path}"),
            Self::CargoConfig(path) => write!(f, "all:cargo-config:{path}"),
            Self::WorkspaceManifest => f.write_str("all:workspace-manifest"),
            Self::UnknownPath(path) => write!(f, "all:unknown-path:{path}"),
            Self::Global(path) => write!(f, "all:global:{path}"),
            Self::GeneratedWorkflow => f.write_str("all:generated-workflow"),
            Self::BaseUnavailable => f.write_str("all:base-unavailable"),
            Self::MostPackages(packages) => write!(f, "all:most-packages:{}", packages.len()),
        }
    }
}
