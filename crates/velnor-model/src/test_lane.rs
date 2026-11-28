// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ids::{PackageName, RepoPath, RunnerLabel};
use crate::repo_config::LaneFeatures;
use crate::test_group::TestNeed;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestLane {
    pub root: Option<RepoPath>,
    pub features: LaneFeatures,
    pub scope: LaneScope,
    pub lint: bool,
    pub doc: bool,
    pub packages: Vec<PackageName>,
    pub job: LaneJob,
    /// The `timeout_minutes` key: the time limit of the lane's own job, or of the root's test job
    /// that runs an inline lane. `None` keeps the root's limit.
    pub timeout_minutes: Option<u16>,
    pub written: LaneKeysWritten,
}

/// Which optional keys of a `[[test_lane]]` table were written, whatever their values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LaneKeysWritten {
    pub job: bool,
    pub feature_flags: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LaneScope {
    #[default]
    Whole,
    Rerun(String),
    Exclusive(String),
}

impl LaneScope {
    #[must_use]
    pub fn filter(&self) -> Option<&str> {
        match self {
            Self::Whole => None,
            Self::Rerun(filter) | Self::Exclusive(filter) => Some(filter),
        }
    }

    #[must_use]
    pub fn exclusive(&self) -> Option<&str> {
        match self {
            Self::Exclusive(filter) => Some(filter),
            Self::Whole | Self::Rerun(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LaneJob {
    #[default]
    Inline,
    Parallel(ParallelLane),
}

impl LaneJob {
    #[must_use]
    pub fn parallel(&self) -> Option<&ParallelLane> {
        match self {
            Self::Inline => None,
            Self::Parallel(lane) => Some(lane),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParallelLane {
    pub runner: Option<LaneRunner>,
    pub needs: Vec<TestNeed>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneRunner {
    pub label: RunnerLabel,
    pub hosted: Option<bool>,
}
