// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ids::{JobId, PathGlob, RepoPath, RootSlug};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintJob {
    pub id: JobId,
    pub name: String,
    pub timeout_minutes: u16,
    pub inputs: Vec<PathGlob>,
    pub kind: LintKind,
}

const JOB_PREFIX: &str = "lint-";

impl LintJob {
    #[must_use]
    pub fn slug(&self) -> RootSlug {
        let tail = self
            .id
            .as_str()
            .strip_prefix(JOB_PREFIX)
            .unwrap_or(self.id.as_str());
        RootSlug::new(
            tail.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .collect::<String>(),
        )
        .unwrap_or_else(|_| RootSlug::from_static("lint"))
    }
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
    GeneratedCheck,
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
