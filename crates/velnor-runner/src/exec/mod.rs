// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod adapter;
mod cancel;
mod commands;
mod context;
mod contexts;
mod encode;
mod env_files;
mod evaluate;
mod execute;
mod host_env;
mod inputs;
mod issues;
mod job;
mod lines;
mod mask;
mod output;
mod preflight;
mod private;
mod shell;
mod sink;
mod step;
mod step_info;
mod timeout;
mod token;

use std::collections::BTreeMap;
use std::path::PathBuf;

pub(crate) use adapter::ProcessOutcome;
pub use adapter::{AdapterRegistry, StepAdapter, StepContext, StepFuture};
pub use cancel::JobCancel;
pub use inputs::Inputs;
pub use job::run_job;
pub use sink::{LogSink, StdoutSink};

use crate::expr::ExprError;

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub workspace: PathBuf,
    pub temp: PathBuf,
    pub runner_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepResult {
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
}

impl StepResult {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) fn status_name(self) -> &'static str {
        match self {
            Self::Succeeded => "success",
            Self::Failed => "failure",
            Self::Skipped => "skipped",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobResult {
    Succeeded,
    Failed,
    Cancelled,
}

impl JobResult {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StepRecord {
    pub number: u32,
    pub context_id: Option<String>,
    pub name: String,
    pub outcome: StepResult,
    pub conclusion: StepResult,
    pub summary_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationLevel {
    Notice,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub level: AnnotationLevel,
    pub message: String,
    pub title: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub end_line: Option<u32>,
    pub column: Option<u32>,
    pub end_column: Option<u32>,
}

impl Annotation {
    pub(crate) fn new(level: AnnotationLevel, message: impl Into<String>) -> Self {
        Self {
            level,
            message: message.into(),
            title: None,
            file: None,
            line: None,
            end_line: None,
            column: None,
            end_column: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct JobRun {
    pub result: JobResult,
    pub steps: Vec<StepRecord>,
    pub outputs: BTreeMap<String, String>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Expression(#[from] ExprError),
    #[error("{0}")]
    FileCommand(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("{0}")]
    Invalid(String),
    #[error("the step was cancelled")]
    Cancelled,
}

impl ExecError {
    pub(crate) fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}
