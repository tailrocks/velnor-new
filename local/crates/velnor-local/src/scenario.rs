// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

pub const DEFAULT_BRANCH: &str = "{default}";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub description: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    Push(String),
    Commit {
        branch: String,
        append: String,
        line: String,
    },
    UpdateLock {
        branch: String,
        package: String,
        #[serde(default)]
        workspace: String,
    },
    OpenPullRequest {
        head: String,
        base: String,
    },
    MergePullRequest(String),
    ClosePullRequest(String),
    MergeGroup(String),
    Schedule(String),
    WorkflowDispatch {
        branch: String,
        #[serde(default)]
        inputs: BTreeMap<String, String>,
    },
}

impl Step {
    fn branches(&mut self) -> Vec<&mut String> {
        match self {
            Self::Push(branch)
            | Self::Commit { branch, .. }
            | Self::UpdateLock { branch, .. }
            | Self::MergePullRequest(branch)
            | Self::ClosePullRequest(branch)
            | Self::MergeGroup(branch)
            | Self::WorkflowDispatch { branch, .. } => vec![branch],
            Self::OpenPullRequest { head, base } => vec![head, base],
            Self::Schedule(_) => Vec::new(),
        }
    }

    fn arguments(&self) -> Vec<&str> {
        match self {
            Self::Push(branch)
            | Self::MergePullRequest(branch)
            | Self::ClosePullRequest(branch)
            | Self::MergeGroup(branch)
            | Self::WorkflowDispatch { branch, .. } => vec![branch],
            Self::Commit { branch, append, .. } => vec![branch, append],
            Self::UpdateLock {
                branch,
                package,
                workspace,
            } => [branch, package]
                .into_iter()
                .chain((!workspace.is_empty()).then_some(workspace))
                .map(String::as_str)
                .collect(),
            Self::OpenPullRequest { head, base } => vec![head, base],
            Self::Schedule(_) => Vec::new(),
        }
    }
}

impl Scenario {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(bytes).map_err(|e| format!("not a scenario: {e}"))
    }

    #[must_use]
    pub fn on_default_branch(mut self, default: &str) -> Self {
        for step in &mut self.steps {
            for branch in step.branches() {
                if branch == DEFAULT_BRANCH {
                    default.clone_into(branch);
                }
            }
        }
        self
    }

    pub fn refuse_options<'a>(
        &'a self,
        values: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), String> {
        let steps = self.steps.iter().flat_map(Step::arguments);
        match values
            .into_iter()
            .chain(steps)
            .find(|value| value.is_empty() || value.starts_with('-'))
        {
            Some(value) => Err(format!("`{value}` is empty or starts with `-`")),
            None => self.refuse_escaping_paths(),
        }
    }

    fn refuse_escaping_paths(&self) -> Result<(), String> {
        let inside = |path: &str| {
            Path::new(path)
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        };
        match self.steps.iter().find_map(|step| match step {
            Step::Commit { append, .. } if !inside(append) => Some(append),
            Step::UpdateLock { workspace, .. } if !workspace.is_empty() && !inside(workspace) => {
                Some(workspace)
            }
            _ => None,
        }) {
            Some(path) => Err(format!("`{path}` is not a path inside the repository")),
            None => Ok(()),
        }
    }
}

#[must_use]
pub fn next_patch(lock: &str, package: &str) -> Option<(String, String)> {
    let block = lock.split("\n\n").find(|block| {
        block
            .lines()
            .any(|line| line == format!("name = \"{package}\""))
    })?;
    let version = block
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))?
        .trim_end_matches('"');
    let (rest, patch) = version.rsplit_once('.')?;
    let next = format!("{rest}.{}", patch.parse::<u64>().ok()? + 1);
    Some((format!("{package}@{version}"), next))
}

#[derive(Debug)]
pub struct Outcome {
    pub id: u64,
    pub workflow: String,
    pub event: String,
    pub conclusion: &'static str,
    pub started: Option<u64>,
    pub metrics: Result<PathBuf, String>,
}

impl Outcome {
    #[must_use]
    pub fn passed(&self) -> bool {
        self.conclusion == "success" && self.metrics.is_ok()
    }

    #[must_use]
    pub fn line(&self) -> String {
        let metrics = match &self.metrics {
            Ok(path) => format!("metrics {}", path.display()),
            Err(error) => format!("no metrics file: {error}"),
        };
        format!(
            "run {} {} ({}): {}, {metrics}",
            self.id, self.workflow, self.event, self.conclusion
        )
    }
}

#[must_use]
pub fn link_inside(link: &Path, target: &Path) -> bool {
    let mut depth = link.components().count().saturating_sub(1);
    for part in target.components() {
        match part {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return false,
        }
    }
    true
}

#[must_use]
pub fn overlapping(state: &Path, checkout: &Path) -> bool {
    state.starts_with(checkout) || checkout.starts_with(state)
}

#[must_use]
pub fn verdict(scenario: &str, outcomes: &[Outcome], up_at: u64, up_seconds: f64) -> String {
    let mut text = format!("\nVerdict of scenario {scenario}\n");
    for outcome in outcomes {
        let _ = writeln!(text, "  {}", outcome.line());
    }
    let first = outcomes.iter().filter_map(|outcome| outcome.started).min();
    let _ = writeln!(
        text,
        "Timing: up in {up_seconds:.1} s, first job started {} after up began.",
        first.map_or_else(
            || "never".to_owned(),
            |at| format!("{} s", at.saturating_sub(up_at))
        )
    );
    text.push_str(
        "Executed: every step of every job above except actions/checkout, by GitHub's official runner \
         in linux/arm64 containers.\n\
         Stood in: runner.server queued the jobs and replaced actions/checkout with a checkout of the local copy; \
         the cache server decided every cache hit, miss and refusal with velnor-cache-rules; \
         the API stub answered GITHUB_API_URL; every Ubuntu label ran on the one Ubuntu 24.04 runner image.\n",
    );
    text
}
