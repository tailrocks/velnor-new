// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The repository, the default branch and one run attempt, as the legacy op re-reads them.

use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::types::{CI_WORKFLOW_PATH, RunMetadata, RunRef};
use crate::error::CiError;
use crate::write_request::internal;

/// One authenticated GET of a GitHub API route (no leading slash). `None` is any failure: a
/// status other than success, a body over the limit, a timeout.
pub trait Api {
    fn get(&self, route: &str) -> Option<Vec<u8>>;
}

pub(super) struct Client<'a> {
    api: &'a dyn Api,
    repository: String,
    default_branch: String,
}

#[derive(Deserialize)]
struct RepositoryResponse {
    full_name: Option<String>,
    default_branch: Option<String>,
}

#[derive(Deserialize)]
struct BranchResponse {
    protected: Option<bool>,
}

#[derive(Deserialize)]
struct RunResponse {
    id: Option<u64>,
    run_attempt: Option<u32>,
    event: Option<String>,
    status: Option<String>,
    conclusion: Option<String>,
    head_branch: Option<String>,
    head_sha: Option<String>,
    path: Option<String>,
    head_repository: Option<HeadRepository>,
}

#[derive(Deserialize)]
struct HeadRepository {
    full_name: Option<String>,
}

pub(super) fn required(value: Option<String>, problem: &str) -> Result<String, CiError> {
    value
        .filter(|text| !text.is_empty())
        .ok_or_else(|| internal(problem))
}

/// The legacy `validate_repository_slug`: no whitespace, `owner/name`, lowercased.
fn repository_slug(raw: &str) -> Option<String> {
    if raw.chars().any(char::is_whitespace) {
        return None;
    }
    let mut parts = raw.split('/');
    let (Some(owner), Some(repo), None) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{}/{}", owner.to_lowercase(), repo.to_lowercase()))
}

fn percent_encode_segment(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl<'a> Client<'a> {
    pub(super) fn for_repository(api: &'a dyn Api, repository: &str) -> Result<Self, CiError> {
        let repository = repository_slug(repository)
            .ok_or_else(|| internal("qualification_repository_invalid"))?;
        let probe = Self {
            api,
            repository: repository.clone(),
            default_branch: String::new(),
        };
        let response: RepositoryResponse = probe.json(&format!("repos/{repository}"))?;
        if response.full_name.as_deref() != Some(repository.as_str()) {
            return Err(internal("qualification_repository_mismatch"));
        }
        let default_branch = required(response.default_branch, "qualification_default_branch")?;
        if default_branch.trim().is_empty()
            || default_branch.starts_with('-')
            || default_branch.contains("..")
            || default_branch.chars().any(char::is_control)
        {
            return Err(internal("qualification_default_branch_invalid"));
        }
        Ok(Self {
            default_branch,
            ..probe
        })
    }

    pub(super) fn repository(&self) -> &str {
        &self.repository
    }

    pub(super) fn bytes(&self, route: &str) -> Result<Vec<u8>, CiError> {
        if route.is_empty() || route.starts_with('/') || route.contains("..") {
            return Err(internal("qualification_api_route_invalid"));
        }
        self.api
            .get(route)
            .ok_or_else(|| internal("qualification_api_unavailable"))
    }

    pub(super) fn json<T: DeserializeOwned>(&self, route: &str) -> Result<T, CiError> {
        serde_json::from_slice(&self.bytes(route)?).map_err(|_| internal("qualification_api_json"))
    }

    pub(super) fn validate_current_branch(&self, expected: &str) -> Result<(), CiError> {
        if expected != self.default_branch {
            return Err(internal("qualification_default_branch_mismatch"));
        }
        let route = format!(
            "repos/{}/branches/{}",
            self.repository,
            percent_encode_segment(expected)
        );
        let response: BranchResponse = self.json(&route)?;
        if response.protected != Some(true) {
            return Err(internal("qualification_default_branch_unprotected"));
        }
        Ok(())
    }

    pub(super) fn run_metadata(&self, run: RunRef) -> Result<RunMetadata, CiError> {
        let route = format!(
            "repos/{}/actions/runs/{}/attempts/{}",
            self.repository, run.run_id, run.run_attempt
        );
        let value: RunResponse = self.json(&route)?;
        let head_sha = required(value.head_sha.clone(), "qualification_run_head_sha")?;
        let event = required(value.event.clone(), "qualification_run_event")?;
        let conclusion = required(value.conclusion.clone(), "qualification_run_conclusion")?;
        let head_branch = required(value.head_branch.clone(), "qualification_run_branch")?;
        let path = required(value.path.clone(), "qualification_run_workflow")?;
        let head_repo = value
            .head_repository
            .as_ref()
            .and_then(|repo| repo.full_name.as_deref())
            .ok_or_else(|| internal("qualification_run_repository"))?;
        let branch = &self.default_branch;
        if !(value.id == Some(run.run_id) && value.run_attempt == Some(run.run_attempt))
            || value.status.as_deref() != Some("completed")
            || event != "workflow_dispatch"
            || conclusion != "success"
            || head_branch != *branch
            || head_repo != self.repository
            || !valid_sha(&head_sha)
            || !workflow_path_matches(&path, branch)
        {
            return Err(internal("qualification_run_authority_mismatch"));
        }
        Ok(RunMetadata {
            repository: self.repository.clone(),
            default_branch: branch.clone(),
            git_ref: format!("refs/heads/{branch}"),
            ref_protected: true,
            workflow_path_ref: format!("{CI_WORKFLOW_PATH}@{branch}"),
            workflow_ref: format!("{}/{CI_WORKFLOW_PATH}@refs/heads/{branch}", self.repository),
            workflow_sha: head_sha.clone(),
            head_sha,
            event,
            conclusion,
            run,
        })
    }
}

fn workflow_path_matches(value: &str, branch: &str) -> bool {
    value == CI_WORKFLOW_PATH
        || value
            .strip_prefix(&format!("{CI_WORKFLOW_PATH}@"))
            .is_some_and(|suffix| suffix == branch || suffix == format!("refs/heads/{branch}"))
}
