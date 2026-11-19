// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::check::workflow;
use crate::error::{WorkflowsError, refuse};
use crate::raw::RawFile;

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
    pub condition: Option<String>,
    pub inputs: Vec<Input>,
    pub jobs: Vec<Job>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: String,
    pub needs: Vec<String>,
    pub helper: bool,
    pub environment: Option<String>,
    pub timeout_minutes: u16,
    pub permissions: Vec<(String, String)>,
    pub outputs: Vec<(String, String)>,
    pub matrix: Option<Matrix>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    pub from_job: String,
    pub from_output: String,
    pub fail_fast: Option<bool>,
    pub max_parallel: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub id: Option<String>,
    pub name: String,
    pub condition: Option<String>,
    pub body: Body,
    pub with: Vec<(String, Env)>,
    pub continue_on_error: bool,
    pub env: Vec<(String, Env)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Run(String),
    Uses(Action),
    Helper(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Checkout,
    Mise,
    UploadArtifact,
    DownloadArtifact,
    CacheRestore,
    CacheSave,
}

impl Action {
    pub const NAMES: [&'static str; 6] = [
        "checkout",
        "mise",
        "upload-artifact",
        "download-artifact",
        "cache-restore",
        "cache-save",
    ];

    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "checkout" => Self::Checkout,
            "mise" => Self::Mise,
            "upload-artifact" => Self::UploadArtifact,
            "download-artifact" => Self::DownloadArtifact,
            "cache-restore" => Self::CacheRestore,
            "cache-save" => Self::CacheSave,
            _ => return None,
        })
    }

    #[must_use]
    pub fn locked_off(self) -> &'static [&'static str] {
        match self {
            Self::Checkout => &["persist-credentials"],
            Self::Mise => &["cache", "cache_save"],
            Self::UploadArtifact
            | Self::DownloadArtifact
            | Self::CacheRestore
            | Self::CacheSave => &[],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Env {
    Text(String),
    Expression(String),
}

impl WorkflowsPlan {
    pub fn read(text: &str) -> Result<Self, WorkflowsError> {
        let raw: RawFile =
            toml::from_str(text).map_err(|error| WorkflowsError(error.to_string()))?;
        if raw.workflow.is_empty() {
            return refuse("declare at least one [[workflow]]");
        }
        let workflows = raw
            .workflow
            .into_iter()
            .map(workflow)
            .collect::<Result<Vec<_>, _>>()?;
        for (index, flow) in workflows.iter().enumerate() {
            if workflows[..index]
                .iter()
                .any(|other| other.file == flow.file)
            {
                return refuse(format!("the workflow file {} is declared twice", flow.file));
            }
        }
        Ok(Self { workflows })
    }
}
