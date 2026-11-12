// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct Repository {
    pub(super) id: u64,
    pub(super) full_name: String,
    pub(super) default_branch: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct Branch {
    pub(super) name: String,
    pub(super) protected: bool,
    pub(super) commit: Commit,
}

#[derive(Debug, Deserialize)]
pub(super) struct Commit {
    pub(super) sha: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct Comparison {
    pub(super) status: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct Run {
    pub(super) id: u64,
    #[serde(rename = "run_attempt")]
    pub(super) attempt: u32,
    pub(super) event: String,
    pub(super) path: String,
    pub(super) head_branch: String,
    pub(super) head_sha: String,
    pub(super) head_repository: Option<HeadRepository>,
    pub(super) status: String,
    pub(super) conclusion: Option<String>,
    #[serde(default)]
    pub(super) referenced_workflows: Vec<ReferencedWorkflow>,
}

#[derive(Debug, Deserialize)]
pub(super) struct HeadRepository {
    pub(super) id: u64,
    pub(super) full_name: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ReferencedWorkflow {
    pub(super) path: String,
    #[serde(rename = "ref")]
    pub(super) reference: Option<String>,
    pub(super) sha: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct Artifact {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) expired: bool,
    pub(super) size_in_bytes: u64,
    pub(super) workflow_run: ArtifactRun,
}

#[derive(Debug, Deserialize)]
pub(super) struct ArtifactRun {
    pub(super) id: u64,
    pub(super) head_sha: String,
    pub(super) head_branch: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct Job {
    pub(super) id: Option<u64>,
    #[serde(default)]
    pub(super) run_id: Option<u64>,
    #[serde(default)]
    pub(super) head_sha: Option<String>,
    pub(super) name: String,
    #[serde(default)]
    pub(super) run_attempt: Option<u32>,
    #[serde(default)]
    pub(super) check_run_url: Option<String>,
    pub(super) status: String,
    pub(super) conclusion: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CiJob {
    pub(super) id: Option<u64>,
    pub(super) run_id: Option<u64>,
    pub(super) name: Option<String>,
    pub(super) head_sha: Option<String>,
    pub(super) status: Option<String>,
    pub(super) conclusion: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowRun {
    pub(super) id: u64,
    pub(super) run_number: u64,
    pub(super) run_attempt: u32,
    pub(super) path: String,
    pub(super) event: String,
    pub(super) head_branch: String,
    pub(super) head_sha: String,
    pub(super) head_repository: Option<HeadRepository>,
    pub(super) status: String,
    pub(super) conclusion: Option<String>,
}
