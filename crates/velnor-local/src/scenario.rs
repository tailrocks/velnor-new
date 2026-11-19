// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Output;

use serde::Deserialize;

pub const DEFAULT_BRANCH: &str = "{default}";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub description: String,
    #[serde(default)]
    pub fresh_cache: bool,
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
        #[serde(default)]
        fork: bool,
    },
    MergePullRequest(String),
    ClosePullRequest(String),
    MergeGroup(String),
    Schedule(String),
    WorkflowDispatch {
        branch: String,
        #[serde(default)]
        inputs: BTreeMap<String, serde_json::Value>,
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
            Self::OpenPullRequest { head, base, .. } => vec![head, base],
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
            Self::OpenPullRequest { head, base, .. } => vec![head, base],
            Self::Schedule(_) => Vec::new(),
        }
    }
}

impl Scenario {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(bytes).map_err(|e| format!("not a scenario: {e}"))
    }

    #[must_use]
    pub fn keeps_cache(&self, fresh_cache: bool) -> bool {
        !fresh_cache && !self.fresh_cache
    }

    #[must_use]
    pub fn has_fork(&self) -> bool {
        self.steps
            .iter()
            .any(|step| matches!(step, Step::OpenPullRequest { fork: true, .. }))
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
        for step in &self.steps {
            if let Step::WorkflowDispatch { inputs, .. } = step {
                let supported = inputs
                    .values()
                    .all(|value| value.is_boolean() || value.is_number() || value.is_string());
                if inputs.len() > 25 || !supported {
                    return Err(
                        "workflow_dispatch inputs are at most 25 booleans, numbers or strings"
                            .to_owned(),
                    );
                }
            }
        }
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
    let numbers: Vec<u64> = version
        .split('.')
        .take(3)
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    let [major, minor, patch] = numbers[..] else {
        return None;
    };
    let next = format!("{major}.{minor}.{}", patch + 1);
    Some((format!("{package}@{version}"), next))
}

#[must_use]
pub fn with_path(list: Option<&str>, path: &str) -> String {
    match list {
        Some(list) if !list.is_empty() => format!("{list}:{path}"),
        _ => path.to_owned(),
    }
}

/// What the velnor-ci report of one run came to. Only `Failed` fails the run:
/// a velnor-ci that is not installed is a warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Metrics {
    Written(PathBuf),
    NotFound,
    Failed(String),
}

/// Maps the result of running `velnor-ci bench collect` to its report.
#[must_use]
pub fn metrics_of(
    program: &Path,
    collected: io::Result<Output>,
    written: bool,
    path: PathBuf,
) -> Metrics {
    match collected {
        Ok(done) if done.status.success() && written => Metrics::Written(path),
        Ok(done) if done.status.success() => Metrics::Failed(format!(
            "{} exited 0 but wrote no {}",
            program.display(),
            path.display()
        )),
        Ok(done) => Metrics::Failed(String::from_utf8_lossy(&done.stderr).trim().to_owned()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Metrics::NotFound,
        Err(error) => Metrics::Failed(format!("{}: {error}", program.display())),
    }
}

#[derive(Debug)]
pub struct Outcome {
    pub id: u64,
    pub workflow: String,
    pub event: String,
    pub conclusion: &'static str,
    pub started: Option<u64>,
    pub metrics: Metrics,
    pub caches: Vec<String>,
    pub trusted: bool,
}

#[derive(Debug, Deserialize)]
struct Restore {
    writer: String,
    key: String,
    matched_key: Option<String>,
    scope: Option<String>,
}

#[must_use]
pub fn restores(journal: &str, run: u64, jobs: &[crate::standin::Job]) -> Vec<String> {
    let prefix = format!("Actions.Results:{run}:");
    journal
        .lines()
        .filter_map(|line| serde_json::from_str::<Restore>(line).ok())
        .filter_map(|restore| {
            let job_id = restore.writer.strip_prefix(&prefix)?;
            let job = jobs.iter().find(|job| job.job_id == job_id)?;
            Some(match (restore.matched_key, restore.scope) {
                (Some(matched), Some(scope)) => format!(
                    "job {}: key {} restored {matched} from {scope}",
                    job.name, restore.key
                ),
                _ => format!("job {}: key {} found no entry", job.name, restore.key),
            })
        })
        .collect()
}

impl Outcome {
    #[must_use]
    pub fn passed(&self) -> bool {
        self.conclusion == "success"
            && (!self.trusted || !matches!(self.metrics, Metrics::Failed(_)))
    }

    #[must_use]
    pub fn line(&self) -> String {
        let metrics = match (&self.metrics, self.trusted) {
            (Metrics::Written(path), _) => format!("metrics {}", path.display()),
            (_, false) => {
                "untrusted, so no metrics file: the API stub cannot see its stand-in".to_owned()
            }
            (Metrics::NotFound, true) => {
                "warning: velnor-ci not found; skipping the report".to_owned()
            }
            (Metrics::Failed(error), true) => format!("no metrics file: {error}"),
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
pub fn inside(root: &Path, dir: &Path) -> bool {
    dir.starts_with(root)
}

pub fn plan(scenarios: &[String], dir: &Path) -> Result<Vec<(String, String, PathBuf)>, String> {
    let mut planned: Vec<(String, String, PathBuf)> = Vec::new();
    for scenario in scenarios {
        let path = Path::new(scenario);
        let stem = match (path.extension(), path.file_stem()) {
            (Some(extension), Some(stem)) if extension == "json" => {
                stem.to_string_lossy().into_owned()
            }
            _ => scenario.clone(),
        };
        let mut parts = Path::new(&stem).components();
        if !matches!(
            (parts.next(), parts.next()),
            (Some(Component::Normal(_)), None)
        ) {
            return Err(format!(
                "{scenario} has no name that can be a directory inside the state directory"
            ));
        }
        if let Some((other, ..)) = planned.iter().find(|(_, taken, _)| *taken == stem) {
            return Err(format!(
                "{other} and {scenario} would share the environment {stem}; give each scenario its own name"
            ));
        }
        let dir = if scenarios.len() > 1 {
            dir.join(&stem)
        } else {
            dir.to_owned()
        };
        planned.push((scenario.clone(), stem, dir));
    }
    Ok(planned)
}

pub fn distinct(planned: &[(String, String, PathBuf)]) -> Result<(), String> {
    for (index, (scenario, _, dir)) in planned.iter().enumerate() {
        if let Some((other, ..)) = planned[..index]
            .iter()
            .find(|(_, _, taken)| overlapping(taken, dir))
        {
            return Err(format!(
                "{other} and {scenario} would share the environment {}; give each scenario its own directory",
                dir.display()
            ));
        }
    }
    Ok(())
}

pub fn summary(results: &[(String, Result<(), String>)]) -> Result<(), String> {
    let failed: Vec<String> = results
        .iter()
        .filter_map(|(name, result)| {
            result
                .as_ref()
                .err()
                .map(|error| format!("{name}: {error}"))
        })
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join("; "))
    }
}

#[must_use]
pub const fn keeps_environment(
    keep: bool,
    interrupted: bool,
    replay_failed: bool,
    runs: usize,
) -> bool {
    keep && !interrupted && (!replay_failed || runs > 0)
}
