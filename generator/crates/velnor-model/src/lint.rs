// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ids::{JobId, RepoPath};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintJob {
    pub id: JobId,
    pub name: String,
    pub timeout_minutes: u16,
    pub kind: LintKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LintKind {
    Jactionlint {
        config: Option<RepoPath>,
    },
    Alint {
        config: RepoPath,
        probe: Option<RepoPath>,
    },
    MiseTask {
        task: String,
        tools: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LintCommand {
    Jactionlint {
        config: Option<RepoPath>,
        shellcheck: bool,
    },
    Alint {
        config: RepoPath,
    },
    Probe(RepoPath),
    MiseTask(String),
}
