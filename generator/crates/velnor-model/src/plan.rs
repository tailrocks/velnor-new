// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::ids::{DownloadUrl, HelperVersion, JobId, RepoPath, RunnerLabel, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowPlan {
    pub name: String,
    pub triggers: Triggers,
    pub concurrency: Concurrency,
    pub permissions: Permissions,
    pub jobs: Vec<Job>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Triggers {
    pub pull_request: bool,
    pub workflow_dispatch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Concurrency {
    pub cancel_in_progress: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Permissions(pub BTreeMap<PermissionScope, Access>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PermissionScope {
    Actions,
    Contents,
    PullRequests,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    None,
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: JobId,
    pub runs_on: RunnerLabel,
    pub timeout_minutes: u16,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub kind: StepKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    Checkout { depth: u32 },
    Cargo(CargoCommand),
    Helper(HelperCommand),
    GeneratorCheck,
    FetchHelper(HelperRelease),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperCommand {
    InputsSave,
    InputsRestore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperRelease {
    pub version: HelperVersion,
    pub binaries: BTreeMap<Platform, HelperBinary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperBinary {
    pub url: DownloadUrl,
    pub sha256: Sha256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Platform {
    LinuxX64,
    LinuxArm64,
    MacosArm64,
}

impl Platform {
    #[must_use]
    pub const fn runner_key(self) -> &'static str {
        match self {
            Self::LinuxX64 => "Linux-X64",
            Self::LinuxArm64 => "Linux-ARM64",
            Self::MacosArm64 => "macOS-ARM64",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CargoCommand {
    FmtCheck { manifest: RepoPath },
}
