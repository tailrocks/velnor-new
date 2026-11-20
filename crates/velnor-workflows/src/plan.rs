// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache_mode::CacheMode;
use crate::check::workflow;
use crate::error::{WorkflowsError, refuse};
use crate::matrix_runner::MatrixRunner;
use crate::raw::RawFile;

mod action;

pub use action::Action;

pub(crate) const DEFAULT_RUNNER: &str = "ubuntu-latest";
pub(crate) const DEFAULT_TIMEOUT_MINUTES: u16 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowsPlan {
    pub workflows: Vec<Workflow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workflow {
    pub file: String,
    pub name: String,
    pub runner: String,
    /// True only when `hosted = true` was declared for `runner`.
    pub hosted: bool,
    pub fork_runner: String,
    pub trusted: Option<TrustedPool>,
    pub condition: Option<String>,
    pub concurrency: Option<Concurrency>,
    pub triggers: Triggers,
    pub jobs: Vec<Job>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedPool {
    pub label: String,
    pub replaces: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Concurrency {
    pub group: String,
    pub cancel_in_progress: bool,
}

/// The events that start a workflow. Every field that is empty, `None` or
/// `false` leaves its event out of the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Triggers {
    pub workflow_call: Option<WorkflowCall>,
    pub workflow_dispatch: Option<Vec<Input>>,
    pub schedule: Vec<String>,
    pub push: Option<Push>,
    pub pull_request: Option<PullRequest>,
    pub merge_group: bool,
    pub workflow_run: Option<WorkflowRun>,
}

impl Triggers {
    /// The names of the events that start the workflow, in the order the file writes them.
    #[must_use]
    pub fn events(&self) -> Vec<&'static str> {
        [
            (self.workflow_call.is_some(), "workflow_call"),
            (self.workflow_dispatch.is_some(), "workflow_dispatch"),
            (!self.schedule.is_empty(), "schedule"),
            (self.push.is_some(), "push"),
            (self.pull_request.is_some(), "pull_request"),
            (self.merge_group, "merge_group"),
            (self.workflow_run.is_some(), "workflow_run"),
        ]
        .into_iter()
        .filter_map(|(present, name)| present.then_some(name))
        .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkflowCall {
    pub inputs: Vec<Input>,
    pub outputs: Vec<Output>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Push {
    pub branches: Vec<String>,
    pub tags: Vec<String>,
    pub paths: Vec<String>,
    pub paths_ignore: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub types: Vec<String>,
    pub branches: Vec<String>,
    pub paths: Vec<String>,
    pub paths_ignore: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRun {
    pub workflows: Vec<String>,
    pub types: Vec<String>,
    pub branches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
    pub kind: InputKind,
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputKind {
    String,
    Boolean,
    Number,
    Choice(Vec<String>),
    Environment,
}

impl InputKind {
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Boolean => "boolean",
            Self::Number => "number",
            Self::Choice(_) => "choice",
            Self::Environment => "environment",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub name: String,
    pub description: Option<String>,
    pub from_job: String,
    pub from_output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: String,
    pub name: Option<String>,
    pub needs: Vec<String>,
    pub condition: Option<String>,
    pub helper: bool,
    pub runner: Option<String>,
    /// Set instead of `runner` when `runs-on` reads a matrix axis.
    pub matrix_runner: Option<MatrixRunner>,
    /// True only when the runner of this job is declared `hosted = true`, or, for a matrix
    /// runner, when every label of the axis is.
    pub hosted: bool,
    pub environment: Option<String>,
    pub timeout_minutes: u16,
    pub concurrency: Option<Concurrency>,
    pub permissions: Vec<(String, String)>,
    pub cache_mode: Option<CacheMode>,
    pub env: Vec<(String, Env)>,
    pub outputs: Vec<(String, String)>,
    pub matrix: Option<Matrix>,
    pub container: Option<Container>,
    pub services: Vec<Service>,
    pub steps: Vec<Step>,
    pub call: Option<Call>,
}

/// A job that calls a reusable workflow instead of running steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub target: CallTarget,
    pub with: Vec<(String, CallValue)>,
    pub outputs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallTarget {
    /// A workflow declared earlier in the same file, by its `file`.
    Declared(String),
    /// `./.github/workflows/<name>.yml` of this repository.
    Local(String),
    /// `owner/repo/.github/workflows/<name>.yml@<full commit SHA>`.
    Pinned(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallValue {
    Text(String),
    Boolean(bool),
    Number(u64),
    Expression(String),
}

/// A job-level `container:`; the image is pinned by digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    pub image: String,
    pub options: Option<String>,
    pub env: Vec<(String, String)>,
}

/// A service container; the image is pinned by digest and `options` carries a health check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub image: String,
    pub ports: Vec<String>,
    pub options: String,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    pub source: MatrixSource,
    pub fail_fast: Option<bool>,
    pub max_parallel: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatrixSource {
    Output {
        from_job: String,
        from_output: String,
    },
    Axes {
        axes: Vec<(String, Vec<String>)>,
        include: Vec<Vec<(String, String)>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub id: Option<String>,
    pub name: String,
    pub condition: Option<String>,
    pub body: Body,
    pub with: Vec<(String, Env)>,
    pub continue_on_error: bool,
    pub working_directory: Option<String>,
    pub env: Vec<(String, Env)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Run(String),
    Uses(Action),
    Local(LocalAction),
    Helper(String),
}

/// A composite, Docker or JavaScript action kept in this repository. Only
/// the qualification fixtures may be used as local actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalAction {
    PostFail,
    Docker,
    Composite,
}

impl LocalAction {
    pub const PATHS: &[&'static str] = &[
        "./qualification/actions/post-fail",
        "./qualification/actions/docker",
        "./qualification/actions/composite",
    ];

    #[must_use]
    pub fn parse(path: &str) -> Option<Self> {
        Some(match path {
            "./qualification/actions/post-fail" => Self::PostFail,
            "./qualification/actions/docker" => Self::Docker,
            "./qualification/actions/composite" => Self::Composite,
            _ => return None,
        })
    }

    #[must_use]
    pub fn path(self) -> &'static str {
        match self {
            Self::PostFail => Self::PATHS[0],
            Self::Docker => Self::PATHS[1],
            Self::Composite => Self::PATHS[2],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Env {
    Text(String),
    Expression(String),
    Interpolated(String),
}

impl WorkflowsPlan {
    pub fn read(text: &str) -> Result<Self, WorkflowsError> {
        let raw: RawFile =
            toml::from_str(text).map_err(|error| WorkflowsError(error.to_string()))?;
        if raw.workflow.is_empty() {
            return refuse("declare at least one [[workflow]]");
        }
        let mut workflows: Vec<Workflow> = Vec::new();
        for flow in raw.workflow {
            let checked = workflow(flow, &workflows)?;
            if workflows.iter().any(|other| other.file == checked.file) {
                return refuse(format!(
                    "the workflow file {} is declared twice",
                    checked.file
                ));
            }
            workflows.push(checked);
        }
        crate::workflow_run::check_chains(&workflows)?;
        Ok(Self { workflows })
    }
}

impl Job {
    /// The output names other jobs may read: the declared outputs of a step
    /// job, or the outputs of the reusable workflow a call job runs.
    pub fn output_names(&self) -> impl Iterator<Item = &str> {
        self.outputs.iter().map(|(name, _)| name.as_str()).chain(
            self.call
                .iter()
                .flat_map(|call| call.outputs.iter().map(String::as_str)),
        )
    }
}
