// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

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
    fn get_artifact_page(
        &self,
        route: &str,
        limit: u64,
    ) -> Result<(Value, u64), OrchestratorError> {
        #[cfg(test)]
        {
            let value = self.get(route)?;
            let length = u64::try_from(
                serde_json::to_vec(&value)
                    .map_err(|_| internal("crate_release_artifact_pages_invalid"))?
                    .len(),
            )
            .map_err(|_| internal("crate_release_artifact_pages_invalid"))?;
            if length == 0 || length > limit {
                return Err(internal("crate_release_artifact_pages_invalid"));
            }
            Ok((value, length))
        }
        #[cfg(not(test))]
        {
            let _ = (route, limit);
            Err(internal("crate_release_artifact_pages_unavailable"))
        }
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError>;
}

#[path = "crate_release_github_native.rs"]
mod native;
pub use native::NativeGitHubAdmissionRead;

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

fn required_workflow_suites(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    source: &str,
    workflow_path: &str,
) -> Result<BTreeSet<u64>, OrchestratorError> {
    let mut suites = BTreeSet::new();
    let mut expected = None;
    let mut seen = 0_u64;
    for page in 1..=100 {
        let value = api.get(&format!(
            "{prefix}/actions/runs?head_sha={source}&per_page=100&page={page}"
        ))?;
        let total = value["total_count"]
            .as_u64()
            .ok_or_else(|| internal("crate_release_runs_total"))?;
        require(total <= 10_000, "runs_limit")?;
        if let Some(previous) = expected {
            require(previous == total, "runs_changed")?;
        }
        expected = Some(total);
        let runs = value["workflow_runs"]
            .as_array()
            .ok_or_else(|| internal("crate_release_runs_invalid"))?;
        seen = seen
            .checked_add(
                u64::try_from(runs.len()).map_err(|_| internal("crate_release_runs_limit"))?,
            )
            .ok_or_else(|| internal("crate_release_runs_limit"))?;
        for run in runs {
            let path = run["path"]
                .as_str()
                .filter(|path| !path.is_empty())
                .ok_or_else(|| internal("crate_release_required_workflow"))?;
            require(
                run["head_sha"].as_str() == Some(source),
                "required_workflow",
            )?;
            if path == workflow_path {
                let id = run["check_suite_id"]
                    .as_u64()
                    .filter(|id| *id > 0)
                    .ok_or_else(|| internal("crate_release_check_suite"))?;
                require(suites.insert(id), "required_workflow")?;
            }
        }
        if seen >= total {
            break;
        }
        require(!runs.is_empty(), "runs_pagination")?;
    }
    require(expected == Some(seen), "runs_pagination")?;
    Ok(suites)
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
    verify_github_evidence_with_check(request, api, None)
}

fn verify_github_evidence_with_check(
    request: &GitHubAdmissionRequest,
    api: &dyn GitHubAdmissionRead,
    recorded_check: Option<u64>,
) -> Result<GitHubCandidate, OrchestratorError> {
    validate_request(request)?;
    let prefix = format!("repos/{}", request.repository);
    verify_run_authority(request, api, &prefix)?;
    verify_environment(request, api, &prefix)?;
    let required_check_id =
        checks::release_required_check(api, request, &request.source_sha, recorded_check)?;
    if request.workflow_sha != request.source_sha {
        checks::release_required_check(api, request, &request.workflow_sha, None)?;
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

pub(crate) fn verify_recorded_github_evidence(
    candidate: &GitHubCandidate,
    api: &dyn GitHubAdmissionRead,
) -> Result<GitHubCandidate, OrchestratorError> {
    let fresh = verify_github_evidence_with_check(
        &candidate.request,
        api,
        Some(candidate.required_check_id),
    )?;
    require(&fresh == candidate, "candidate_changed")?;
    Ok(fresh)
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
            intent.exact_source_sha().map(str::to_owned)
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
    let mut candidate = verify_github_evidence(&request, api)?;
    candidate.required_check_id = receipt.required_check_id;
    require(
        receipt.trusted_reviewer_id == intent.trusted_reviewer_id,
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
pub(crate) use tags::release_is_prerelease;
pub use tags::{GitHubReleaseEvidence, read_github_release_evidence, verify_immutable_tag_rules};
