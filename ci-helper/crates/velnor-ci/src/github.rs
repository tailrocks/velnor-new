// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::time::Duration;

use serde::Deserialize;

use crate::error::CiError;

pub trait GitHub {
    fn get(&self, path: &str) -> Result<String, CiError>;
}

pub trait GitHubWrite: GitHub {
    fn wait(&self, duration: Duration);
    fn upload(&self, url: &str, bytes: &[u8]) -> Result<String, CiError>;
    fn patch(&self, path: &str, json: &str) -> Result<String, CiError>;
    fn delete(&self, path: &str) -> Result<(), CiError>;
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
    #[serde(default)]
    pub labels: Vec<String>,
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
    pub id: u64,
    pub default_branch: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiRepoId {
    pub id: u64,
}

#[derive(Debug, Deserialize)]
pub struct ApiCompare {
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiRule {
    #[serde(rename = "type")]
    pub kind: String,
    pub parameters: Option<ApiRuleParameters>,
    pub ruleset_id: Option<u64>,
    pub ruleset_source: Option<String>,
    pub ruleset_source_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApiRuleParameters {
    pub required_status_checks: Option<Vec<ApiRequiredCheck>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiRequiredCheck {
    pub context: String,
    #[serde(alias = "app_id")]
    pub integration_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ApiBranch {
    pub protection: ApiProtection,
}

#[derive(Debug, Deserialize)]
pub struct ApiProtection {
    pub required_status_checks: ApiClassicChecks,
}

#[derive(Debug, Deserialize)]
pub struct ApiClassicChecks {
    pub enforcement_level: String,
    pub contexts: Vec<String>,
    pub checks: Vec<ApiRequiredCheck>,
}

#[derive(Debug, Deserialize)]
pub struct ApiPage<T> {
    pub total_count: usize,
    #[serde(
        rename = "items",
        alias = "workflow_runs",
        alias = "jobs",
        alias = "check_runs",
        alias = "check_suites",
        alias = "statuses"
    )]
    pub items: Vec<T>,
}

#[derive(Debug, Deserialize)]
pub struct ApiWorkflowRun {
    pub id: u64,
    pub event: String,
    pub status: String,
    pub head_branch: Option<String>,
    pub head_sha: String,
    pub head_repository: Option<ApiRepoId>,
}

#[derive(Debug, Deserialize)]
pub struct ApiMergeGroupRun {
    pub event: String,
    pub status: String,
    pub head_sha: String,
    pub conclusion: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApiListedRun {
    pub id: u64,
    pub event: String,
    pub run_attempt: u32,
    pub head_branch: Option<String>,
    #[serde(default)]
    pub pull_requests: Vec<ApiListedPullRequest>,
}

#[derive(Debug, Deserialize)]
pub struct ApiListedPullRequest {
    pub base: ApiBase,
}

#[derive(Debug, Deserialize)]
pub struct ApiBase {
    #[serde(rename = "ref")]
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiCommitStatus {
    pub context: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiRunJob {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
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
pub fn commit_pulls_path(repo: &Repo, sha: &str) -> String {
    format!("/repos/{repo}/commits/{}/pulls?per_page=100", encode(sha))
}

#[must_use]
pub fn pull_commits_path(repo: &Repo, number: u64) -> String {
    format!("/repos/{repo}/pulls/{number}/commits")
}

#[must_use]
pub fn tag_ref_path(repo: &Repo, tag: &str) -> String {
    format!("/repos/{repo}/git/ref/tags/{}", encode(tag))
}

#[must_use]
pub fn tag_object_path(repo: &Repo, sha: &str) -> String {
    format!("/repos/{repo}/git/tags/{sha}")
}

#[must_use]
pub fn rules_path(repo: &Repo, branch: &str, page: u32) -> String {
    format!(
        "/repos/{repo}/rules/branches/{}?per_page=100&page={page}",
        encode(branch)
    )
}

#[must_use]
pub fn branch_path(repo: &Repo, branch: &str) -> String {
    format!("/repos/{repo}/branches/{}", encode(branch))
}

#[must_use]
pub fn statuses_path(repo: &Repo, sha: &str, page: u32) -> String {
    format!("/repos/{repo}/commits/{sha}/status?per_page=100&page={page}")
}

#[must_use]
pub fn runs_path(repo: &Repo, sha: &str, page: u32) -> String {
    format!(
        "/repos/{repo}/actions/runs?head_sha={sha}&exclude_pull_requests=true&per_page=100&page={page}"
    )
}

#[must_use]
pub fn merge_group_runs_path(repo: &Repo, workflow_file: &str, sha: &str) -> String {
    format!(
        "/repos/{repo}/actions/workflows/{}/runs?head_sha={}&event=merge_group&status=success&per_page=100",
        encode(workflow_file),
        encode(sha)
    )
}

#[must_use]
pub fn run_jobs_path(repo: &Repo, run_id: u64, page: u32) -> String {
    format!("/repos/{repo}/actions/runs/{run_id}/jobs?filter=latest&per_page=100&page={page}")
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

#[must_use]
pub fn workflow_runs_path(repo: &Repo, workflow_file: &str, page: u32) -> String {
    format!(
        "/repos/{repo}/actions/workflows/{}/runs?status=success&per_page=100&page={page}",
        encode(workflow_file)
    )
}
