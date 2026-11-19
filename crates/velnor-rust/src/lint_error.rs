// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LintFault {
    ToolUnpinned(String),
    ToolAmbiguous { tool: String, keys: Vec<String> },
    ToolFiltered { tool: String, platform: String },
    ToolLockStale { tool: String, version: String },
    TaskMissing(String),
    TaskToolUndeclared { task: String, tool: String },
    NodeRequired,
}

impl fmt::Display for LintFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ToolUnpinned(tool) => write!(
                f,
                "lint needs {tool}, which mise.toml does not pin; pin it there and run `mise lock`"
            ),
            Self::ToolAmbiguous { tool, keys } => write!(
                f,
                "lint names {tool}, which matches {} in mise.toml; name one of them in full",
                keys.join(" and ")
            ),
            Self::ToolFiltered { tool, platform } => write!(
                f,
                "lint needs {tool}, but every request for it in mise.toml is filtered out by os on {platform}; pin a version that applies there"
            ),
            Self::ToolLockStale { tool, version } => write!(
                f,
                "mise.lock has no entry for {tool} at {version}; run `mise lock`"
            ),
            Self::TaskMissing(task) => write!(
                f,
                "lint runs the mise task {task}, which neither [tasks] in mise.toml nor a mise task folder defines"
            ),
            Self::TaskToolUndeclared { task, tool } => write!(
                f,
                "the mise task {task} uses {tool}, which lint.tools does not name; the job installs only the tools it names, so the task would fall back to whatever the runner has"
            ),
            Self::NodeRequired => f.write_str(
                "a workflow uses actions/github-script, and jactionlint checks its script only with node on PATH; pin node in mise.toml and run `mise lock`",
            ),
        }
    }
}
