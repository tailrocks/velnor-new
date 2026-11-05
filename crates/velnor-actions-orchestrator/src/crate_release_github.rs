// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{OrchestratorError, internal::internal};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubAdmissionRequest {
    pub repository: String,
    pub default_branch: String,
    pub environment: String,
    pub trusted_reviewer_id: u64,
    pub workflow_path: String,
    pub required_workflow_path: String,
    pub workflow_sha: String,
    pub run_id: u64,
    pub run_attempt: u32,
    pub event: String,
    pub source_sha: String,
    pub recovery_sha: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubCandidate {
    pub request: GitHubAdmissionRequest,
    pub required_check_id: u64,
    pub release_pr: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubAdmissionOutcome {
    Candidate(Box<GitHubCandidate>),
    Skipped,
}

pub trait GitHubAdmissionRead {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError>;
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError>;
}

pub struct NativeGitHubAdmissionRead {
    pub(crate) reader: crate::crate_release_http::GitHubAdmissionHttp,
}

impl std::fmt::Debug for NativeGitHubAdmissionRead {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeGitHubAdmissionRead")
            .finish_non_exhaustive()
    }
}

impl NativeGitHubAdmissionRead {
    pub fn new(token: String) -> Result<Self, OrchestratorError> {
        require(!token.is_empty(), "github_token_missing")?;
        let timeout = Duration::from_secs(180);
        Ok(Self {
            reader: crate::crate_release_http::GitHubAdmissionHttp::new(token, timeout)?,
        })
    }
}

impl crate::crate_release_controller::CrateReleaseRepairWrite for NativeGitHubAdmissionRead {
    fn create_tag(
        &self,
        repository: &str,
        tag: &str,
        source_sha: &str,
    ) -> Result<(), OrchestratorError> {
        self.reader.create_tag(repository, tag, source_sha)
    }
    fn create_release(
        &self,
        repository: &str,
        tag: &str,
        source_sha: &str,
    ) -> Result<(), OrchestratorError> {
        self.reader.create_release(repository, tag, source_sha)
    }
}

impl GitHubAdmissionRead for NativeGitHubAdmissionRead {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        self.reader.get(route)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        self.reader.get_optional(route)
    }
}

impl crate::crate_release_qualification_witness::WitnessRead for NativeGitHubAdmissionRead {
    fn witness_job_log(&self, repository: &str, job_id: u64) -> Result<Vec<u8>, OrchestratorError> {
        self.reader.qualification_job_log(repository, job_id)
    }

    fn artifact_bytes(
        &self,
        repository: &str,
        artifact_id: u64,
        expected_size: u64,
        expected_sha256: &str,
    ) -> Result<Vec<u8>, OrchestratorError> {
        self.reader.qualification_artifact_bytes(
            repository,
            artifact_id,
            expected_size,
            expected_sha256,
        )
    }
}

impl crate::github_review_feedback::FeedbackRead for NativeGitHubAdmissionRead {
    fn feedback_get(&self, route: &str) -> Result<Value, OrchestratorError> {
        self.reader.get(route)
    }

    fn feedback_graphql(
        &self,
        query: crate::github_review_feedback::FeedbackQuery,
        fields: &[(&str, String)],
    ) -> Result<Value, OrchestratorError> {
        let mut values = BTreeMap::new();
        for (name, value) in fields {
            if !matches!(*name, "owner" | "name" | "number" | "cursor")
                || values.insert(*name, value.as_str()).is_some()
            {
                return Err(internal("qualification_feedback_fields"));
            }
        }
        let owner = values
            .get("owner")
            .ok_or_else(|| internal("qualification_feedback_fields"))?;
        let name = values
            .get("name")
            .ok_or_else(|| internal("qualification_feedback_fields"))?;
        let number = values
            .get("number")
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|n| *n > 0)
            .ok_or_else(|| internal("qualification_feedback_fields"))?;
        let repository = format!("{owner}/{name}");
        let cursor = values.get("cursor").copied();
        match query {
            crate::github_review_feedback::FeedbackQuery::Threads => self
                .reader
                .qualification_review_threads(&repository, number, cursor),
            crate::github_review_feedback::FeedbackQuery::Opinions => self
                .reader
                .qualification_opinionated_reviews(&repository, number, cursor),
        }
    }
}

impl crate::tap_merge::review_completion::provider::ProviderRead for NativeGitHubAdmissionRead {
    fn provider_app(
        &self,
        app: crate::tap_merge::review_completion::provider::ProviderApp,
    ) -> Result<Value, OrchestratorError> {
        self.reader.get(app.route())
    }
    fn provider_graphql(
        &self,
        query: crate::tap_merge::review_completion::provider::ProviderQuery,
        fields: &[(&str, String)],
    ) -> Result<Value, OrchestratorError> {
        self.reader.qualification_provider_graphql(query, fields)
    }
    fn provider_check_runs(
        &self,
        repository: &str,
        head: &str,
    ) -> Result<Vec<Value>, OrchestratorError> {
        crate::crate_release_qualification_checks::complete_check_runs(self, repository, head)
    }
}

pub fn admit_github_evidence(
    request: &GitHubAdmissionRequest,
    api: &dyn GitHubAdmissionRead,
) -> Result<GitHubAdmissionOutcome, OrchestratorError> {
    validate_request(request)?;
    if request.event == "push" {
        require(
            request.recovery_sha.is_none() && request.source_sha == request.workflow_sha,
            "push_source",
        )?;
        let prefix = format!("repos/{}", request.repository);
        verify_run_authority(request, api, &prefix)?;
        if !has_release_pr(api, &prefix, &request.source_sha)? {
            return Ok(GitHubAdmissionOutcome::Skipped);
        }
    }
    verify_github_evidence(request, api)
        .map(|value| GitHubAdmissionOutcome::Candidate(Box::new(value)))
}

pub(crate) fn has_release_pr(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    source: &str,
) -> Result<bool, OrchestratorError> {
    for page in 1..=100 {
        let response = api.get(&format!(
            "{prefix}/commits/{source}/pulls?per_page=100&page={page}"
        ))?;
        let prs = response
            .as_array()
            .ok_or_else(|| internal("crate_release_pulls_invalid"))?;
        for pr in prs {
            let branch = pr["head"]["ref"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| internal("crate_release_pull_branch"))?;
            if branch.starts_with("release-plz-") {
                return Ok(true);
            }
        }
        if prs.len() < 100 {
            return Ok(false);
        }
    }
    Err(internal("crate_release_pulls_limit"))
}

pub(crate) fn verify_required_check(
    api: &dyn GitHubAdmissionRead,
    repository: &str,
    sha: &str,
    workflow_path: &str,
) -> Result<u64, OrchestratorError> {
    required_check(api, &format!("repos/{repository}"), sha, workflow_path)
}

pub fn verify_github_admission(
    request: &GitHubAdmissionRequest,
    token: String,
) -> Result<GitHubCandidate, OrchestratorError> {
    validate_request(request)?;
    let api = NativeGitHubAdmissionRead::new(token)?;
    verify_github_evidence(request, &api)
}

pub fn verify_github_evidence(
    request: &GitHubAdmissionRequest,
    api: &dyn GitHubAdmissionRead,
) -> Result<GitHubCandidate, OrchestratorError> {
    validate_request(request)?;
    let prefix = format!("repos/{}", request.repository);
    verify_run_authority(request, api, &prefix)?;
    verify_environment(request, api, &prefix)?;
    let required_check_id = required_check(
        api,
        &prefix,
        &request.source_sha,
        &request.required_workflow_path,
    )?;
    if request.workflow_sha != request.source_sha {
        required_check(
            api,
            &prefix,
            &request.workflow_sha,
            &request.required_workflow_path,
        )?;
    }
    let release_pr = if request.event == "push" {
        require(
            request.recovery_sha.is_none() && request.source_sha == request.workflow_sha,
            "push_source",
        )?;
        Some(approved_pr(api, &prefix, request, &request.source_sha)?)
    } else {
        require(
            request.recovery_sha.as_deref() == Some(request.source_sha.as_str()),
            "recovery_source",
        )?;
        None
    };
    Ok(GitHubCandidate {
        request: request.clone(),
        required_check_id,
        release_pr,
    })
}

pub(crate) fn verify_predecessor_authority(
    api: &dyn GitHubAdmissionRead,
    current: &GitHubAdmissionRequest,
    receipt: &crate::crate_release_journal::JournalReceipt,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<(), OrchestratorError> {
    let identity = &receipt.identity;
    require(identity.run_attempt == 1, "approval_attempt_unverified")?;
    let prefix = format!("repos/{}", identity.repository);
    let repository = api.get(&prefix)?;
    let id = repository["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("crate_release_previous_repository"))?;
    let run = api.get(&format!(
        "{prefix}/actions/runs/{}/attempts/{}",
        identity.run_id, identity.run_attempt
    ))?;
    let event = run["event"]
        .as_str()
        .filter(|event| matches!(*event, "push" | "workflow_dispatch"))
        .ok_or_else(|| internal("crate_release_previous_event"))?;
    let request = GitHubAdmissionRequest {
        repository: identity.repository.clone(),
        default_branch: intent.default_branch.clone(),
        environment: intent.environment.clone(),
        trusted_reviewer_id: intent.trusted_reviewer_id,
        workflow_path: current.workflow_path.clone(),
        required_workflow_path: intent.required_workflow_path.clone(),
        workflow_sha: identity.workflow_sha.clone(),
        run_id: identity.run_id,
        run_attempt: identity.run_attempt,
        event: event.to_owned(),
        source_sha: identity.source_sha.clone(),
        recovery_sha: if event == "workflow_dispatch" {
            intent
                .recovery
                .as_ref()
                .map(|value| value.source_sha.clone())
                .or_else(|| {
                    intent
                        .new_crates
                        .as_ref()
                        .map(|value| value.source_sha.clone())
                })
        } else {
            None
        },
    };
    require(
        run["id"].as_u64() == Some(identity.run_id)
            && run["run_attempt"].as_u64() == Some(u64::from(identity.run_attempt))
            && run["status"].as_str() == Some("completed")
            && run["head_sha"].as_str() == Some(identity.workflow_sha.as_str())
            && run["head_branch"].as_str() == Some(request.default_branch.as_str())
            && run["event"].as_str() == Some(request.event.as_str())
            && run["path"].as_str() == Some(request.workflow_path.as_str())
            && run["repository"]["id"].as_u64() == Some(id)
            && run["head_repository"]["id"].as_u64() == Some(id)
            && run["repository"]["full_name"].as_str() == Some(identity.repository.as_str())
            && run["head_repository"]["full_name"].as_str() == Some(identity.repository.as_str()),
        "previous_run",
    )?;
    let candidate = verify_github_evidence(&request, api)?;
    require(
        candidate.required_check_id == receipt.required_check_id
            && receipt.trusted_reviewer_id == intent.trusted_reviewer_id,
        "previous_admission",
    )?;
    authorize_github_candidate(&request, &candidate, api)?;
    Ok(())
}

#[path = "crate_release_github_authority.rs"]
mod authority;
use authority::{verify_environment, verify_run_authority};

#[path = "crate_release_github_authorization.rs"]
mod authorization;

#[path = "crate_release_github_checks.rs"]
mod checks;
use checks::{approved_pr, required_check};

#[path = "crate_release_github_reviews.rs"]
mod reviews;
use reviews::{encode, require, valid_sha, valid_utc, validate_request, verify_reviews};

#[path = "crate_release_github_tags.rs"]
mod tags;

pub use authorization::{
    GitHubAuthorized, authorize_github_candidate, read_github_tag, read_github_tag_evidence,
    verify_environment_approval,
};
pub use tags::{GitHubReleaseEvidence, read_github_release_evidence, verify_immutable_tag_rules};
