// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use serde::Deserialize;

use crate::error::CiError;

pub trait GitHub {
    fn get(&self, path: &str) -> Result<String, CiError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    owner: String,
    name: String,
}

impl Repo {
    pub fn parse(text: &str) -> Result<Self, CiError> {
        match text.split_once('/') {
            Some((owner, name)) if !owner.is_empty() && !name.is_empty() && !name.contains('/') => {
                Ok(Self {
                    owner: owner.to_owned(),
                    name: name.to_owned(),
                })
            }
            _ => Err(CiError::new(format!(
                "repository must be owner/name, got `{text}`"
            ))),
        }
    }
}

impl fmt::Display for Repo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

#[derive(Debug, Deserialize)]
pub struct ApiRun {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub run_attempt: u32,
    pub event: String,
    pub head_sha: String,
    pub created_at: String,
    pub run_started_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiJobs {
    pub total_count: usize,
    pub jobs: Vec<ApiJob>,
}

#[derive(Debug, Deserialize)]
pub struct ApiJob {
    pub id: u64,
    pub name: String,
    pub conclusion: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    #[serde(default)]
    pub steps: Vec<ApiStep>,
}

#[derive(Debug, Deserialize)]
pub struct ApiStep {
    pub name: String,
    pub conclusion: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApiRepo {
    pub default_branch: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiCompare {
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiCheckRuns {
    pub total_count: usize,
    pub check_runs: Vec<ApiCheckRun>,
}

#[derive(Debug, Deserialize)]
pub struct ApiCheckRun {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub app: Option<ApiApp>,
}

#[derive(Debug, Deserialize)]
pub struct ApiApp {
    pub slug: String,
}

pub fn parse<T: serde::de::DeserializeOwned>(body: &str, what: &str) -> Result<T, CiError> {
    serde_json::from_str(body)
        .map_err(|e| CiError::new(format!("cannot read GitHub {what} response: {e}")))
}

#[must_use]
pub fn repo_path(repo: &Repo) -> String {
    format!("/repos/{repo}")
}

#[must_use]
pub fn compare_path(repo: &Repo, base: &str, head: &str) -> String {
    format!("/repos/{repo}/compare/{}...{head}?per_page=1", encode(base))
}

#[must_use]
pub fn check_runs_path(repo: &Repo, sha: &str, check: &str) -> String {
    format!(
        "/repos/{repo}/commits/{sha}/check-runs?check_name={}&filter=latest&per_page=100",
        encode(check)
    )
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

#[must_use]
pub fn run_path(repo: &Repo, run_id: u64) -> String {
    format!("/repos/{repo}/actions/runs/{run_id}")
}

#[must_use]
pub fn jobs_path(repo: &Repo, run_id: u64, attempt: u32, page: u32) -> String {
    format!("/repos/{repo}/actions/runs/{run_id}/attempts/{attempt}/jobs?per_page=100&page={page}")
}

#[must_use]
pub fn job_log_path(repo: &Repo, job_id: u64) -> String {
    format!("/repos/{repo}/actions/jobs/{job_id}/logs")
}
