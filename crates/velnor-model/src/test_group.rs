// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ids::{JobId, PathGlob, RepoPath, RunnerLabel};
use crate::repo::RustRoot;
use crate::selection::EventKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestGroup {
    pub name: JobId,
    pub filter: String,
    pub needs: Vec<TestNeed>,
    pub test_threads: Option<u16>,
    pub timeout_minutes: Option<u16>,
    pub runner: Option<RunnerLabel>,
    /// The `hosted` key written next to `runner`, if any.
    pub hosted: Option<bool>,
    pub paths: Vec<PathGlob>,
    pub events: Vec<GroupEvent>,
    pub root: GroupRoot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TestNeed {
    Docker,
    Cargo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupEvent {
    PullRequest,
    Push,
    MergeGroup,
    Schedule,
    WorkflowDispatch,
}

impl GroupEvent {
    #[must_use]
    pub fn kind(self) -> EventKind {
        match self {
            Self::PullRequest => EventKind::PullRequest,
            Self::Push => EventKind::Push,
            Self::MergeGroup => EventKind::MergeGroup,
            Self::Schedule => EventKind::Schedule,
            Self::WorkflowDispatch => EventKind::WorkflowDispatch,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupRoot {
    OnlyRoot,
    Folder(Option<RepoPath>),
}

impl TestGroup {
    #[must_use]
    pub fn selects(&self) -> bool {
        !self.paths.is_empty() || !self.events.is_empty()
    }

    #[must_use]
    pub fn belongs_to(&self, root: &RustRoot, root_count: usize) -> bool {
        match &self.root {
            GroupRoot::OnlyRoot => root_count == 1,
            GroupRoot::Folder(folder) => *folder == root.manifest.parent(),
        }
    }
}
