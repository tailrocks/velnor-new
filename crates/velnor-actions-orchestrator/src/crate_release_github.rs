// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

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
        let deadline = Instant::now() + Duration::from_secs(180);
        Ok(Self {
            reader: crate::crate_release_http::GitHubAdmissionHttp::new(token, deadline)?,
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

fn has_release_pr(
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

fn verify_run_authority(
    request: &GitHubAdmissionRequest,
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
) -> Result<(), OrchestratorError> {
    let repo: Value = api.get(prefix)?;
    require(
        repo["full_name"].as_str() == Some(request.repository.as_str()),
        "repository",
    )?;
    require(
        repo["default_branch"].as_str() == Some(request.default_branch.as_str()),
        "default_branch",
    )?;
    let run: Value = api.get(&format!("{prefix}/actions/runs/{}", request.run_id))?;
    require(run["id"].as_u64() == Some(request.run_id), "run_id")?;
    require(
        run["run_attempt"].as_u64() == Some(u64::from(request.run_attempt)),
        "run_attempt",
    )?;
    require(
        run["head_sha"].as_str() == Some(request.workflow_sha.as_str()),
        "workflow_sha",
    )?;
    require(
        run["head_branch"].as_str() == Some(request.default_branch.as_str()),
        "workflow_branch",
    )?;
    require(
        run["event"].as_str() == Some(request.event.as_str()),
        "event",
    )?;
    require(
        run["path"].as_str() == Some(request.workflow_path.as_str()),
        "workflow_path",
    )?;
    require(
        run["repository"]["full_name"].as_str() == Some(request.repository.as_str()),
        "workflow_repository",
    )?;
    require(
        run["head_repository"]["full_name"].as_str() == Some(request.repository.as_str()),
        "workflow_head_repository",
    )?;
    let compare: Value = api.get(&format!(
        "{prefix}/compare/{}...{}",
        request.source_sha, request.workflow_sha
    ))?;
    require(
        matches!(compare["status"].as_str(), Some("ahead" | "identical")),
        "source_ancestry",
    )?;
    let workflow_compare: Value = api.get(&format!(
        "{prefix}/compare/{}...{}",
        request.workflow_sha,
        encode(&request.default_branch)
    ))?;
    require(
        matches!(
            workflow_compare["status"].as_str(),
            Some("ahead" | "identical")
        ),
        "workflow_ancestry",
    )?;
    Ok(())
}

fn verify_environment(
    request: &GitHubAdmissionRequest,
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
) -> Result<(), OrchestratorError> {
    let env: Value = api.get(&format!(
        "{prefix}/environments/{}",
        encode(&request.environment)
    ))?;
    require(
        env["name"].as_str() == Some(request.environment.as_str()),
        "environment",
    )?;
    require(
        env["deployment_branch_policy"]["custom_branch_policies"].as_bool() == Some(true),
        "environment_policy",
    )?;
    require(
        env["can_admins_bypass"].as_bool() == Some(false),
        "environment_bypass",
    )?;
    let protections = env["protection_rules"]
        .as_array()
        .ok_or_else(|| internal("crate_release_environment_protection"))?;
    let reviewers: Vec<&Value> = protections
        .iter()
        .filter(|rule| rule["type"].as_str() == Some("required_reviewers"))
        .collect();
    require(reviewers.len() == 1, "environment_reviewer_rule")?;
    let allowed = reviewers[0]["reviewers"]
        .as_array()
        .ok_or_else(|| internal("crate_release_environment_reviewers"))?;
    require(
        allowed.len() == 1
            && allowed[0]["type"].as_str() == Some("User")
            && allowed[0]["reviewer"]["id"].as_u64() == Some(request.trusted_reviewer_id),
        "environment_reviewers",
    )?;

    let policies: Value = api.get(&format!(
        "{prefix}/environments/{}/deployment-branch-policies?per_page=100",
        encode(&request.environment)
    ))?;
    require(
        policies["total_count"].as_u64() == Some(1),
        "environment_policy_count",
    )?;
    let rules = policies["branch_policies"]
        .as_array()
        .ok_or_else(|| internal("crate_release_environment_rules"))?;
    require(
        rules.len() == 1
            && rules[0]["type"].as_str() == Some("branch")
            && rules[0]["name"].as_str() == Some(request.default_branch.as_str()),
        "environment_branch",
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubAuthorized {
    candidate: GitHubCandidate,
}

impl GitHubAuthorized {
    #[must_use]
    pub fn candidate(&self) -> &GitHubCandidate {
        &self.candidate
    }
}

pub fn authorize_github_candidate(
    request: &GitHubAdmissionRequest,
    candidate: &GitHubCandidate,
    api: &dyn GitHubAdmissionRead,
) -> Result<GitHubAuthorized, OrchestratorError> {
    let fresh = verify_github_evidence(request, api)?;
    require(&fresh == candidate, "candidate_changed")?;
    verify_environment_approval(request, api)?;
    Ok(GitHubAuthorized { candidate: fresh })
}

pub fn verify_environment_approval(
    request: &GitHubAdmissionRequest,
    api: &dyn GitHubAdmissionRead,
) -> Result<(), OrchestratorError> {
    validate_request(request)?;
    require(request.run_attempt == 1, "approval_attempt_unverified")?;
    let prefix = format!("repos/{}", request.repository);
    verify_run_authority(request, api, &prefix)?;
    verify_environment(request, api, &prefix)?;
    let environment = api.get(&format!(
        "{prefix}/environments/{}",
        encode(&request.environment)
    ))?;
    let id = environment["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("crate_release_environment_id"))?;
    let response = api.get(&format!(
        "{prefix}/actions/runs/{}/approvals",
        request.run_id
    ))?;
    let approvals = response
        .as_array()
        .ok_or_else(|| internal("crate_release_approvals_invalid"))?;
    let related: Vec<&Value> = approvals
        .iter()
        .filter(|approval| {
            approval["environments"].as_array().is_some_and(|envs| {
                envs.iter().any(|env| {
                    env["id"].as_u64() == Some(id)
                        && env["name"].as_str() == Some(request.environment.as_str())
                })
            })
        })
        .collect();
    require(related.len() == 1, "environment_approval_count")?;
    let approval = related[0];
    let expected = format!(
        "I reviewed the complete release policy at {} and source {}, and authorize this source for run {}.",
        request.workflow_sha, request.source_sha, request.run_id
    );
    require(
        approval["state"].as_str() == Some("approved")
            && approval["user"]["type"].as_str() == Some("User")
            && approval["user"]["id"].as_u64() == Some(request.trusted_reviewer_id)
            && approval["comment"].as_str() == Some(expected.as_str()),
        "environment_approval",
    )
}

pub fn read_github_tag(
    repository: &str,
    tag: &str,
    token: String,
) -> Result<Option<String>, OrchestratorError> {
    let api = NativeGitHubAdmissionRead::new(token)?;
    read_github_tag_evidence(repository, tag, &api)
}

pub fn read_github_tag_evidence(
    repository: &str,
    tag: &str,
    api: &dyn GitHubAdmissionRead,
) -> Result<Option<String>, OrchestratorError> {
    let Some((owner, repo)) = repository.split_once('/') else {
        return Err(internal("crate_release_repository"));
    };
    require(
        velnor_actions_contract::config::is_valid_github_owner(owner)
            && velnor_actions_contract::config::is_valid_github_repo_name(repo),
        "repository",
    )?;
    require(
        !tag.is_empty()
            && !tag.starts_with('.')
            && !tag.ends_with('.')
            && tag.rsplit_once('.').is_none_or(|(_, ext)| ext != "lock")
            && !tag.contains("..")
            && tag
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'+')),
        "tag",
    )?;
    let prefix = format!("repos/{repository}");
    let repository_info: Value = api.get(&prefix)?;
    require(
        repository_info["full_name"].as_str() == Some(repository),
        "repository",
    )?;
    let branch = repository_info["default_branch"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| internal("crate_release_default_branch"))?;
    let branch_ref = api.get(&format!("{prefix}/git/ref/heads/{}", encode(branch)))?;
    require(
        branch_ref["ref"].as_str() == Some(format!("refs/heads/{branch}").as_str())
            && branch_ref["object"]["type"].as_str() == Some("commit")
            && branch_ref["object"]["sha"].as_str().is_some_and(valid_sha),
        "branch_ref",
    )?;
    let Some(reference): Option<Value> =
        api.get_optional(&format!("{prefix}/git/ref/tags/{}", encode(tag)))?
    else {
        return Ok(None);
    };
    require(
        reference["ref"].as_str() == Some(format!("refs/tags/{tag}").as_str()),
        "tag_reference",
    )?;
    let mut object = reference["object"].clone();
    for _ in 0..=8 {
        let sha = object["sha"]
            .as_str()
            .filter(|s| valid_sha(s))
            .ok_or_else(|| internal("crate_release_tag_sha"))?;
        match object["type"].as_str() {
            Some("commit") => return Ok(Some(sha.to_owned())),
            Some("tag") => {
                let annotated: Value = api.get(&format!("{prefix}/git/tags/{sha}"))?;
                require(annotated["sha"].as_str() == Some(sha), "tag_object")?;
                object = annotated["object"].clone();
            }
            _ => return Err(internal("crate_release_tag_type")),
        }
    }
    Err(internal("crate_release_tag_depth"))
}

struct CheckDecision {
    id: u64,
    suite: u64,
    status: String,
    conclusion: String,
    started: String,
}

fn required_check(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    source: &str,
    workflow_path: &str,
) -> Result<u64, OrchestratorError> {
    let mut latest: Option<CheckDecision> = None;
    let mut seen = 0_u64;
    let mut seen_ids = BTreeSet::new();
    let mut expected = None;
    for page in 1..=100 {
        let result: Value = api.get(&format!(
            "{prefix}/commits/{source}/check-runs?filter=all&per_page=100&page={page}"
        ))?;
        let total = result["total_count"]
            .as_u64()
            .ok_or_else(|| internal("crate_release_checks_total"))?;
        require(total <= 10_000, "checks_limit")?;
        if let Some(previous) = expected {
            require(previous == total, "checks_changed")?;
        }
        expected = Some(total);
        let checks = result["check_runs"]
            .as_array()
            .ok_or_else(|| internal("crate_release_checks_invalid"))?;
        seen = seen
            .checked_add(
                u64::try_from(checks.len()).map_err(|_| internal("crate_release_checks_limit"))?,
            )
            .ok_or_else(|| internal("crate_release_checks_limit"))?;
        for check in checks {
            let check_id = check["id"]
                .as_u64()
                .filter(|id| *id > 0)
                .ok_or_else(|| internal("crate_release_check_id"))?;
            require(seen_ids.insert(check_id), "checks_duplicate")?;
            if check["name"].as_str() != Some("Required") {
                continue;
            }
            require(check["head_sha"].as_str() == Some(source), "check_source")?;
            require(
                check["app"]["id"].as_u64() == Some(15368)
                    && check["app"]["slug"].as_str() == Some("github-actions"),
                "check_app",
            )?;
            let id = check["id"]
                .as_u64()
                .ok_or_else(|| internal("crate_release_check_id"))?;
            let suite = check["check_suite"]["id"]
                .as_u64()
                .ok_or_else(|| internal("crate_release_check_suite"))?;
            let (status, started) = check_status_and_time(check)?;
            if let Some(previous) = &latest {
                if previous.started == started {
                    require(previous.id == id, "check_order_ambiguous")?;
                    continue;
                }
                if previous.started.as_str() > started {
                    continue;
                }
            }
            latest = Some(CheckDecision {
                id,
                suite,
                status: status.to_owned(),
                conclusion: check["conclusion"].as_str().unwrap_or_default().to_owned(),
                started: started.to_owned(),
            });
        }
        if seen >= total {
            break;
        }
        require(!checks.is_empty(), "checks_pagination")?;
    }
    require(expected == Some(seen), "checks_pagination")?;
    let latest = latest.ok_or_else(|| internal("crate_release_required_pending"))?;
    if latest.status != "completed" {
        return Err(internal("crate_release_required_pending"));
    }
    require(
        latest.status == "completed" && latest.conclusion == "success",
        "required_failed",
    )?;
    verify_required_workflow(api, prefix, source, workflow_path, latest.suite)?;
    Ok(latest.id)
}

fn check_status_and_time(check: &Value) -> Result<(&str, &str), OrchestratorError> {
    let status = check["status"]
        .as_str()
        .ok_or_else(|| internal("crate_release_check_status"))?;
    require(
        matches!(
            status,
            "queued" | "in_progress" | "waiting" | "requested" | "pending" | "completed"
        ),
        "check_status",
    )?;
    if status != "completed" && check["started_at"].is_null() {
        return Err(internal("crate_release_required_pending"));
    }
    let started = check["started_at"]
        .as_str()
        .filter(|value| valid_utc(value))
        .ok_or_else(|| internal("crate_release_check_time"))?;
    Ok((status, started))
}

fn verify_required_workflow(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    source: &str,
    workflow_path: &str,
    suite: u64,
) -> Result<(), OrchestratorError> {
    let mut found = false;
    let mut seen_runs = 0_u64;
    let mut expected_runs = None;
    for page in 1..=100 {
        let response = api.get(&format!(
            "{prefix}/actions/runs?head_sha={source}&per_page=100&page={page}"
        ))?;
        let total = response["total_count"]
            .as_u64()
            .ok_or_else(|| internal("crate_release_runs_total"))?;
        require(total <= 10_000, "runs_limit")?;
        if let Some(previous) = expected_runs {
            require(previous == total, "runs_changed")?;
        }
        expected_runs = Some(total);
        let runs = response["workflow_runs"]
            .as_array()
            .ok_or_else(|| internal("crate_release_runs_invalid"))?;
        seen_runs = seen_runs
            .checked_add(
                u64::try_from(runs.len()).map_err(|_| internal("crate_release_runs_limit"))?,
            )
            .ok_or_else(|| internal("crate_release_runs_limit"))?;
        for run in runs {
            if run["check_suite_id"].as_u64() != Some(suite) {
                continue;
            }
            require(
                !found
                    && run["head_sha"].as_str() == Some(source)
                    && run["path"].as_str() == Some(workflow_path)
                    && run["status"].as_str() == Some("completed")
                    && run["conclusion"].as_str() == Some("success"),
                "required_workflow",
            )?;
            found = true;
        }
        if seen_runs >= total {
            break;
        }
        require(!runs.is_empty(), "runs_pagination")?;
    }
    require(
        expected_runs == Some(seen_runs) && found,
        "required_workflow_missing",
    )?;
    Ok(())
}

fn approved_pr(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    request: &GitHubAdmissionRequest,
    commit_sha: &str,
) -> Result<u64, OrchestratorError> {
    let mut candidates = Vec::new();
    for page in 1..=100 {
        let response = api.get(&format!(
            "{prefix}/commits/{commit_sha}/pulls?per_page=100&page={page}"
        ))?;
        let prs = response
            .as_array()
            .ok_or_else(|| internal("crate_release_pulls_invalid"))?;
        let count = prs.len();
        for pr in prs {
            if pr["merged_at"].is_null() || pr["merge_commit_sha"].as_str() != Some(commit_sha) {
                continue;
            }
            if pr["base"]["ref"].as_str() != Some(request.default_branch.as_str())
                || pr["base"]["repo"]["full_name"].as_str() != Some(request.repository.as_str())
            {
                continue;
            }
            if pr["head"]["repo"]["full_name"].as_str() != Some(request.repository.as_str())
                || !pr["head"]["ref"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("release-plz-"))
            {
                continue;
            }
            candidates.push(pr.clone());
        }
        if count < 100 {
            break;
        }
        require(page < 100, "pulls_limit")?;
    }
    require(candidates.len() == 1, "release_pr")?;
    let pr = &candidates[0];
    let number = pr["number"]
        .as_u64()
        .ok_or_else(|| internal("crate_release_pr_number"))?;
    let author = pr["user"]["login"]
        .as_str()
        .ok_or_else(|| internal("crate_release_pr_author"))?;
    let head = pr["head"]["sha"]
        .as_str()
        .ok_or_else(|| internal("crate_release_pr_head"))?;
    require(valid_sha(head), "release_pr_head")?;
    let merged = pr["merged_at"]
        .as_str()
        .filter(|value| valid_utc(value))
        .ok_or_else(|| internal("crate_release_merge_time"))?;
    let source_commit = api.get(&format!("{prefix}/git/commits/{commit_sha}"))?;
    let head_commit = api.get(&format!("{prefix}/git/commits/{head}"))?;
    let source_tree = source_commit["tree"]["sha"]
        .as_str()
        .filter(|s| valid_sha(s))
        .ok_or_else(|| internal("crate_release_source_tree"))?;
    require(
        source_commit["sha"].as_str() == Some(commit_sha)
            && head_commit["sha"].as_str() == Some(head)
            && head_commit["tree"]["sha"].as_str() == Some(source_tree),
        "reviewed_source_tree",
    )?;
    verify_reviews(api, prefix, number, author, head, merged)?;
    Ok(number)
}

struct ReviewDecision {
    id: u64,
    user_id: u64,
    state: String,
    commit: String,
    association: String,
    submitted: String,
    human: bool,
}

fn verify_reviews(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    number: u64,
    author: &str,
    head: &str,
    merged: &str,
) -> Result<(), OrchestratorError> {
    let latest = collect_review_decisions(api, prefix, number, merged)?;
    require(
        !latest
            .values()
            .any(|decision| decision.state == "CHANGES_REQUESTED"),
        "release_pr_changes_requested",
    )?;
    for (login, decision) in latest {
        if !decision.human
            || login.eq_ignore_ascii_case(author)
            || decision.state != "APPROVED"
            || decision.commit != head
            || !matches!(
                decision.association.as_str(),
                "OWNER" | "MEMBER" | "COLLABORATOR"
            )
        {
            continue;
        }
        let permission = api.get(&format!(
            "{prefix}/collaborators/{}/permission",
            encode(&login)
        ))?;
        if permission["user"]["id"].as_u64() == Some(decision.user_id)
            && permission["user"]["type"].as_str() == Some("User")
            && matches!(permission["permission"].as_str(), Some("write" | "admin"))
        {
            return Ok(());
        }
    }
    Err(internal("crate_release_release_pr_approval"))
}

fn collect_review_decisions(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    number: u64,
    merged: &str,
) -> Result<BTreeMap<String, ReviewDecision>, OrchestratorError> {
    let mut latest: BTreeMap<String, ReviewDecision> = BTreeMap::new();
    for page in 1..=100 {
        let response = api.get(&format!(
            "{prefix}/pulls/{number}/reviews?per_page=100&page={page}"
        ))?;
        let reviews = response
            .as_array()
            .ok_or_else(|| internal("crate_release_reviews_invalid"))?;
        let count = reviews.len();
        for review in reviews {
            let Some(login) = review["user"]["login"].as_str() else {
                return Err(internal("crate_release_review_user"));
            };
            let human = match review["user"]["type"].as_str() {
                Some("User") => true,
                Some("Bot") => false,
                _ => return Err(internal("crate_release_review_user_type")),
            };
            let user_id = review["user"]["id"]
                .as_u64()
                .filter(|value| *value > 0)
                .ok_or_else(|| internal("crate_release_review_user_id"))?;
            let association = review["author_association"].as_str().unwrap_or_default();
            let id = review["id"]
                .as_u64()
                .ok_or_else(|| internal("crate_release_review_id"))?;
            let state = review["state"].as_str().unwrap_or_default();
            if state == "PENDING" || state == "COMMENTED" {
                continue;
            }
            let submitted = review["submitted_at"]
                .as_str()
                .filter(|value| valid_utc(value))
                .ok_or_else(|| internal("crate_release_review_time"))?;
            if submitted >= merged {
                continue;
            }
            let commit = review["commit_id"].as_str().unwrap_or_default();
            let login = login.to_ascii_lowercase();
            if let Some(previous) = latest.get(&login) {
                if previous.submitted == submitted {
                    require(previous.id == id, "review_order_ambiguous")?;
                    continue;
                }
                if previous.submitted.as_str() > submitted {
                    continue;
                }
            }
            latest.insert(
                login,
                ReviewDecision {
                    id,
                    user_id,
                    state: state.to_owned(),
                    commit: commit.to_owned(),
                    association: association.to_owned(),
                    submitted: submitted.to_owned(),
                    human,
                },
            );
        }
        if count < 100 {
            break;
        }
        require(page < 100, "reviews_limit")?;
    }
    Ok(latest)
}

fn validate_request(request: &GitHubAdmissionRequest) -> Result<(), OrchestratorError> {
    let Some((owner, repo)) = request.repository.split_once('/') else {
        return Err(internal("crate_release_repository"));
    };
    require(
        velnor_actions_contract::config::is_valid_github_owner(owner)
            && velnor_actions_contract::config::is_valid_github_repo_name(repo),
        "repository",
    )?;
    require(
        !request.default_branch.is_empty()
            && request.default_branch.len() <= 255
            && !request.default_branch.contains(['?', '#', '\\']),
        "default_branch",
    )?;
    require(
        !request.environment.is_empty()
            && request.environment.len() <= 255
            && !request.environment.contains(['?', '#', '\\']),
        "environment",
    )?;
    require(
        valid_workflow_path(&request.workflow_path)
            && valid_workflow_path(&request.required_workflow_path),
        "workflow_path",
    )?;
    require(
        valid_sha(&request.workflow_sha) && valid_sha(&request.source_sha),
        "sha",
    )?;
    require(
        request.run_id > 0 && request.run_attempt > 0 && request.trusted_reviewer_id > 0,
        "run",
    )?;
    require(
        matches!(request.event.as_str(), "push" | "workflow_dispatch"),
        "event",
    )?;
    Ok(())
}

fn valid_utc(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19) || byte.is_ascii_digit()
        })
}

fn valid_workflow_path(value: &str) -> bool {
    value
        .strip_prefix(".github/workflows/")
        .is_some_and(|name| {
            name.rsplit_once('.')
                .is_some_and(|(_, ext)| matches!(ext, "yml" | "yaml"))
                && !name.starts_with('.')
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn require(value: bool, reason: &str) -> Result<(), OrchestratorError> {
    if value {
        Ok(())
    } else {
        Err(internal(&format!("crate_release_{reason}")))
    }
}

fn encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubReleaseEvidence {
    pub id: u64,
    pub url: String,
}

pub fn read_github_release_evidence(
    repository: &str,
    tag: &str,
    api: &dyn GitHubAdmissionRead,
) -> Result<Option<GitHubReleaseEvidence>, OrchestratorError> {
    let Some(release) =
        api.get_optional(&format!("repos/{repository}/releases/tags/{}", encode(tag)))?
    else {
        return Ok(None);
    };
    let id = release["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("crate_release_release_id"))?;
    let url = release["html_url"]
        .as_str()
        .ok_or_else(|| internal("crate_release_release_url"))?;
    let raw = format!("https://github.com/{repository}/releases/tag/{tag}");
    let encoded = format!(
        "https://github.com/{repository}/releases/tag/{}",
        encode(tag)
    );
    require(
        release["tag_name"].as_str() == Some(tag)
            && release["draft"].as_bool() == Some(false)
            && (url == raw || url == encoded),
        "release_identity",
    )?;
    Ok(Some(GitHubReleaseEvidence {
        id,
        url: url.to_owned(),
    }))
}

pub fn verify_immutable_tag_rules(
    repository: &str,
    api: &dyn GitHubAdmissionRead,
) -> Result<(), OrchestratorError> {
    let listed = api.get(&format!(
        "repos/{repository}/rulesets?targets=tag&includes_parents=true&per_page=100"
    ))?;
    let list = listed
        .as_array()
        .ok_or_else(|| internal("crate_release_tag_rules_unverified"))?;
    if list.len() >= 100 {
        return Err(internal("crate_release_tag_rules_pagination"));
    }
    let mut protected = false;
    let mut ids = std::collections::BTreeSet::new();
    for item in list {
        let id = item
            .get("id")
            .and_then(Value::as_u64)
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("crate_release_tag_rules_unverified"))?;
        if !ids.insert(id) {
            return Err(internal("crate_release_tag_rules_unverified"));
        }
        let rule = api.get(&format!("repos/{repository}/rulesets/{id}"))?;
        if rule.get("id").and_then(Value::as_u64) != Some(id) {
            return Err(internal("crate_release_tag_rules_unverified"));
        }
        let bypass = rule
            .get("bypass_actors")
            .and_then(Value::as_array)
            .ok_or_else(|| internal("crate_release_tag_rules_unverified"))?;
        let reference = rule
            .pointer("/conditions/ref_name")
            .ok_or_else(|| internal("crate_release_tag_rules_unverified"))?;
        let includes = reference
            .get("include")
            .and_then(Value::as_array)
            .ok_or_else(|| internal("crate_release_tag_rules_unverified"))?;
        let excludes = reference
            .get("exclude")
            .and_then(Value::as_array)
            .ok_or_else(|| internal("crate_release_tag_rules_unverified"))?;
        let rules = rule
            .get("rules")
            .and_then(Value::as_array)
            .ok_or_else(|| internal("crate_release_tag_rules_unverified"))?;
        if rule.get("target").and_then(Value::as_str) == Some("tag")
            && rule.get("enforcement").and_then(Value::as_str) == Some("active")
            && bypass.is_empty()
            && excludes.is_empty()
            && includes.iter().any(|value| value.as_str() == Some("~ALL"))
            && rules.iter().any(|value| {
                value.get("type").and_then(Value::as_str) == Some("update")
                    && (value.get("parameters").is_none()
                        || value
                            .pointer("/parameters/update_allows_fetch_and_merge")
                            .and_then(Value::as_bool)
                            == Some(false))
            })
            && rules
                .iter()
                .any(|value| value.get("type").and_then(Value::as_str) == Some("deletion"))
        {
            protected = true;
        }
    }
    if !protected {
        return Err(internal("crate_release_tag_rules_unverified"));
    }
    Ok(())
}
