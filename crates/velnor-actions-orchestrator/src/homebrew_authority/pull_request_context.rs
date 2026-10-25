//! Captured PR identity shared by ordinary and fork review contexts.

use serde_json::Value;
use std::path::Path;
use velnor_actions_contract::WorkflowEvent;
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

#[path = "pull_request_validation.rs"]
mod validation;
use validation::{merge_parents_match, valid_sha};

#[derive(Clone)]
pub(super) struct PullRequestSelection {
    number: u64,
    event: WorkflowEvent,
    base_sha: String,
    head_sha: String,
    merge_commit_sha: Option<String>,
    checkout_head: String,
    repository: String,
    runner_sha: String,
    runner_ref: String,
    payload_base_sha: String,
    payload_head_sha: String,
    payload_head_repository: String,
    payload_head_repository_id: u64,
    payload_base_repository: String,
    payload_base_repository_id: u64,
    payload_base_ref: String,
    payload_head_ref: String,
    payload_repository_id: u64,
}

#[derive(Clone, Copy)]
pub(super) struct AdmissionRequest<'a> {
    pub(super) root: &'a Path,
    pub(super) repository: &'a str,
    pub(super) repository_id: u64,
    pub(super) event: WorkflowEvent,
    pub(super) number: Option<u64>,
    pub(super) expected_head: &'a str,
    pub(super) checkout_head: &'a str,
}

#[derive(Clone, Copy)]
pub(super) struct RecheckRequest<'a> {
    pub(super) root: &'a Path,
    pub(super) repository: &'a str,
    pub(super) repository_id: u64,
    pub(super) event: WorkflowEvent,
    pub(super) number: Option<u64>,
    pub(super) expected_head: &'a str,
    pub(super) selection: &'a PullRequestSelection,
}

impl PullRequestSelection {
    pub(super) fn base_sha(&self) -> &str {
        &self.base_sha
    }

    pub(super) fn checkout_head(&self) -> &str {
        &self.checkout_head
    }
}

pub(super) fn authenticate(
    request: &AdmissionRequest<'_>,
    deadline: CheckDeadline,
) -> Result<PullRequestSelection, OrchestratorError> {
    let AdmissionRequest {
        root,
        repository,
        repository_id,
        event,
        number,
        expected_head,
        checkout_head,
    } = *request;
    let number = number
        .filter(|number| *number > 0)
        .ok_or_else(|| internal("homebrew_authority_pull_request_missing"))?;
    let runner = capture_runner(event, number, repository)?;
    let pull = crate::package_release_publish::read_public_pull_request(
        repository,
        repository_id,
        number,
        deadline,
    )?;
    let (base_sha, head_sha) = admit_snapshot(
        &pull,
        event,
        repository,
        repository_id,
        expected_head,
        checkout_head,
        &runner,
    )?;
    crate::package_release_publish::verify_public_pull_request_lineage(
        repository,
        repository_id,
        "main",
        &base_sha,
        &head_sha,
        deadline,
    )?;
    verify_checkout(root, &pull, &base_sha, &head_sha, checkout_head, deadline)?;
    Ok(PullRequestSelection {
        number,
        event,
        base_sha,
        head_sha,
        merge_commit_sha: pull.merge_commit_sha.clone(),
        checkout_head: checkout_head.to_owned(),
        repository: repository.to_owned(),
        runner_sha: runner.sha,
        runner_ref: runner.git_ref,
        payload_base_sha: runner.payload_base_sha,
        payload_head_sha: runner.payload_head_sha,
        payload_head_repository: runner.payload_head_repository,
        payload_head_repository_id: runner.payload_head_repository_id,
        payload_base_repository: runner.payload_base_repository,
        payload_base_repository_id: runner.payload_base_repository_id,
        payload_base_ref: runner.payload_base_ref,
        payload_head_ref: runner.payload_head_ref,
        payload_repository_id: runner.payload_repository_id,
    })
}

pub(super) fn recheck(
    request: &RecheckRequest<'_>,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let RecheckRequest {
        root,
        repository,
        repository_id,
        event,
        number,
        expected_head,
        selection,
    } = *request;
    if number != Some(selection.number)
        || event != selection.event
        || repository != selection.repository
    {
        return Err(internal(
            "homebrew_authority_pull_request_selection_changed",
        ));
    }
    let runner = capture_runner(event, selection.number, repository)?;
    if runner.sha != selection.runner_sha
        || runner.git_ref != selection.runner_ref
        || runner.payload_base_sha != selection.payload_base_sha
        || runner.payload_head_sha != selection.payload_head_sha
        || runner.payload_head_repository != selection.payload_head_repository
        || runner.payload_head_repository_id != selection.payload_head_repository_id
        || runner.payload_base_repository != selection.payload_base_repository
        || runner.payload_base_repository_id != selection.payload_base_repository_id
        || runner.payload_base_ref != selection.payload_base_ref
        || runner.payload_head_ref != selection.payload_head_ref
        || runner.payload_repository_id != selection.payload_repository_id
    {
        return Err(internal("homebrew_authority_pull_request_runner_changed"));
    }
    let pull = crate::package_release_publish::read_public_pull_request(
        repository,
        repository_id,
        selection.number,
        deadline,
    )?;
    let (base_sha, head_sha) = admit_snapshot(
        &pull,
        event,
        repository,
        repository_id,
        expected_head,
        &selection.checkout_head,
        &runner,
    )?;
    if !same_selection(
        selection,
        &base_sha,
        &head_sha,
        pull.merge_commit_sha.as_deref(),
    ) {
        return Err(internal("homebrew_authority_pull_request_changed"));
    }
    crate::package_release_publish::verify_public_pull_request_lineage(
        repository,
        repository_id,
        "main",
        &selection.base_sha,
        &selection.head_sha,
        deadline,
    )?;
    verify_checkout(
        root,
        &pull,
        &selection.base_sha,
        &selection.head_sha,
        &selection.checkout_head,
        deadline,
    )?;
    Ok(())
}

fn admit_snapshot(
    pull: &crate::package_release_publish::PublicPullRequestSnapshot,
    event: WorkflowEvent,
    repository: &str,
    repository_id: u64,
    expected_head: &str,
    checkout_head: &str,
    runner: &RunnerFacts,
) -> Result<(String, String), OrchestratorError> {
    super::super::admit_pull_request_context(
        pull,
        event,
        repository,
        repository_id,
        expected_head,
        checkout_head,
        runner,
    )
}

fn same_selection(
    selection: &PullRequestSelection,
    base_sha: &str,
    head_sha: &str,
    merge_commit_sha: Option<&str>,
) -> bool {
    selection.base_sha == base_sha
        && selection.head_sha == head_sha
        && selection.merge_commit_sha.as_deref() == merge_commit_sha
}

pub(in crate::homebrew::authority) struct RunnerFacts {
    pub(in crate::homebrew::authority) sha: String,
    pub(in crate::homebrew::authority) git_ref: String,
    pub(in crate::homebrew::authority) payload_base_sha: String,
    pub(in crate::homebrew::authority) payload_head_sha: String,
    pub(in crate::homebrew::authority) payload_head_repository: String,
    pub(in crate::homebrew::authority) payload_head_repository_id: u64,
    pub(in crate::homebrew::authority) payload_base_repository: String,
    pub(in crate::homebrew::authority) payload_base_repository_id: u64,
    pub(in crate::homebrew::authority) payload_base_ref: String,
    pub(in crate::homebrew::authority) payload_head_ref: String,
    pub(in crate::homebrew::authority) payload_repository_id: u64,
}

struct RunnerEnvironment {
    event_name: String,
    repository: String,
    sha: String,
    git_ref: String,
}

fn capture_runner(
    event: WorkflowEvent,
    number: u64,
    repository: &str,
) -> Result<RunnerFacts, OrchestratorError> {
    let environment = RunnerEnvironment {
        event_name: std::env::var("GITHUB_EVENT_NAME").unwrap_or_default(),
        repository: std::env::var("GITHUB_REPOSITORY").unwrap_or_default(),
        sha: std::env::var("GITHUB_SHA").unwrap_or_default(),
        git_ref: std::env::var("GITHUB_REF").unwrap_or_default(),
    };
    let event_path = std::env::var_os("GITHUB_EVENT_PATH")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("homebrew_authority_pull_request_event_missing"))?;
    let bytes = crate::safe_read::read_event_file(
        Path::new(&event_path),
        crate::safe_read::MAX_REPO_FILE_BYTES,
    )?;
    let payload: Value = serde_json::from_str(&bytes)
        .map_err(|_| internal("homebrew_authority_pull_request_event_invalid"))?;
    parse_runner(event, number, repository, environment, &payload)
}

fn parse_runner(
    event: WorkflowEvent,
    number: u64,
    repository: &str,
    environment: RunnerEnvironment,
    payload: &Value,
) -> Result<RunnerFacts, OrchestratorError> {
    let expected_event = match event {
        WorkflowEvent::PullRequest | WorkflowEvent::Fork => "pull_request",
        _ => return Err(internal("homebrew_authority_pull_request_event")),
    };
    let RunnerEnvironment {
        event_name,
        repository: runner_repository,
        sha,
        git_ref,
    } = environment;
    let pull = &payload["pull_request"];
    let payload_number = pull["number"].as_u64();
    let payload_repository = payload["repository"]["full_name"].as_str();
    let payload_repository_id = payload["repository"]["id"].as_u64();
    let base_sha = pull["base"]["sha"].as_str();
    let head_sha = pull["head"]["sha"].as_str();
    let head_repository = pull["head"]["repo"]["full_name"].as_str();
    let head_repository_id = pull["head"]["repo"]["id"].as_u64();
    let head_ref = pull["head"]["ref"].as_str();
    let base_repository = pull["base"]["repo"]["full_name"].as_str();
    let base_repository_id = pull["base"]["repo"]["id"].as_u64();
    let base_ref = pull["base"]["ref"].as_str();
    let expected_ref = format!("refs/pull/{number}/merge");
    let fork = pull["head"]["repo"]["fork"].as_bool();
    let fork_matches = validation::fork_matches(
        event,
        fork,
        head_repository,
        head_repository_id,
        payload_repository,
        payload_repository_id,
    );
    if event_name != expected_event
        || runner_repository != repository
        || payload_number != Some(number)
        || payload_repository != Some(repository)
        || payload_repository_id.is_none_or(|id| id == 0)
        || !fork_matches
        || !valid_sha(Some(sha.as_str()))
        || !valid_sha(base_sha)
        || !valid_sha(head_sha)
        || head_repository
            .is_none_or(|value| !velnor_actions_contract::vcs::is_safe_github_repository(value))
        || head_repository_id.is_none_or(|value| value == 0)
        || head_ref.is_none_or(str::is_empty)
        || git_ref != expected_ref
    {
        return Err(internal("homebrew_authority_pull_request_runner_context"));
    }
    Ok(RunnerFacts {
        sha,
        git_ref,
        payload_base_sha: base_sha.unwrap_or_default().to_owned(),
        payload_head_sha: head_sha.unwrap_or_default().to_owned(),
        payload_head_repository: head_repository.unwrap_or_default().to_owned(),
        payload_head_repository_id: head_repository_id.unwrap_or_default(),
        payload_base_repository: base_repository.unwrap_or_default().to_owned(),
        payload_base_repository_id: base_repository_id.unwrap_or_default(),
        payload_base_ref: base_ref.unwrap_or_default().to_owned(),
        payload_head_ref: head_ref.unwrap_or_default().to_owned(),
        payload_repository_id: payload_repository_id.unwrap_or_default(),
    })
}

fn verify_checkout(
    root: &Path,
    pull: &crate::package_release_publish::PublicPullRequestSnapshot,
    base_sha: &str,
    head_sha: &str,
    checkout_head: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if crate::homebrew::authority::read_git_value(root, "HEAD", deadline)? != checkout_head {
        return Err(internal("homebrew_authority_pull_request_checkout_changed"));
    }
    if checkout_head == head_sha {
        return Ok(());
    }
    if pull.merge_commit_sha.as_deref() != Some(checkout_head) {
        return Err(internal("homebrew_authority_pull_request_merge_sha"));
    }
    let request =
        velnor_actions_mise::GitRequest::rev_parse(vec![std::ffi::OsString::from("HEAD^@")]);
    let output = request
        .run_in_until(root, deadline)
        .map_err(|_| internal("homebrew_authority_pull_request_merge_parents"))?;
    output
        .require_success("git")
        .map_err(|_| internal("homebrew_authority_pull_request_merge_parents"))?;
    let parents = output
        .stdout_text("git")
        .map_err(|_| internal("homebrew_authority_pull_request_merge_parents"))?
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if !merge_parents_match(&parents, base_sha, head_sha) {
        return Err(internal("homebrew_authority_pull_request_merge_parents"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "pull_request_runner_tests.rs"]
mod runner_tests;
#[cfg(test)]
#[path = "pull_request_snapshot_git_tests.rs"]
mod snapshot_git_tests;
#[cfg(test)]
#[path = "pull_request_context_tests.rs"]
mod tests;
