// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::package_release_publish::MergeGroupSelection;
use serde_json::Value;
use std::collections::BTreeMap;
use velnor_actions_contract::vcs::VcsInputs;

use crate::{OrchestratorError, internal::internal};
use velnor_actions_mise::CheckDeadline;

const EVENT_LIMIT: u64 = crate::safe_read::MAX_REPO_FILE_BYTES;

pub(super) fn authenticate(
    repository: &str,
    repository_id: u64,
    expected_head: &str,
    pull_request: Option<u64>,
    deadline: CheckDeadline,
) -> Result<MergeGroupSelection, OrchestratorError> {
    if pull_request.is_some() {
        return Err(internal("homebrew_authority_unexpected_pull_request"));
    }
    let selection = capture(repository, repository_id, expected_head)?;
    crate::package_release_publish::verify_public_merge_group_context(&selection, deadline)?;
    Ok(selection)
}

pub(super) fn capture(
    repository: &str,
    repository_id: u64,
    expected_head: &str,
) -> Result<MergeGroupSelection, OrchestratorError> {
    let event_name = std::env::var("GITHUB_EVENT_NAME").unwrap_or_default();
    let event_path = std::env::var_os("GITHUB_EVENT_PATH")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("homebrew_merge_group_event_missing"))?;
    let event_text =
        crate::safe_read::read_event_file(std::path::Path::new(&event_path), EVENT_LIMIT)?;
    let event: Value = serde_json::from_str(&event_text)
        .map_err(|_| internal("homebrew_merge_group_event_invalid"))?;
    let runner = RunnerContext {
        repository: std::env::var("GITHUB_REPOSITORY").ok(),
        sha: std::env::var("GITHUB_SHA").ok(),
        git_ref: std::env::var("GITHUB_REF").ok(),
        run_id: std::env::var("GITHUB_RUN_ID").ok(),
        run_attempt: std::env::var("GITHUB_RUN_ATTEMPT").ok(),
    };
    parse(
        repository,
        repository_id,
        expected_head,
        &event_name,
        &runner,
        &event,
    )
}

pub(crate) struct RunnerContext {
    pub(crate) repository: Option<String>,
    pub(crate) sha: Option<String>,
    pub(crate) git_ref: Option<String>,
    pub(crate) run_id: Option<String>,
    pub(crate) run_attempt: Option<String>,
}

pub(crate) fn parse(
    expected_repository: &str,
    repository_id: u64,
    expected_head: &str,
    event_name: &str,
    runner: &RunnerContext,
    event: &Value,
) -> Result<MergeGroupSelection, OrchestratorError> {
    if event_name != "merge_group"
        || !velnor_actions_contract::vcs::is_safe_github_repository(expected_repository)
        || repository_id == 0
        || !lowercase_sha(expected_head)
        || runner.repository.as_deref() != Some(expected_repository)
        || runner.sha.as_deref() != Some(expected_head)
    {
        return Err(internal("homebrew_merge_group_runner_context"));
    }
    let group = &event["merge_group"];
    let repository = &event["repository"];
    let base_sha = group["base_sha"]
        .as_str()
        .ok_or_else(|| internal("homebrew_merge_group_base_sha"))?;
    let head_sha = group["head_sha"]
        .as_str()
        .ok_or_else(|| internal("homebrew_merge_group_head_sha"))?;
    let base_ref = group["base_ref"]
        .as_str()
        .ok_or_else(|| internal("homebrew_merge_group_base_ref"))?;
    let head_ref = group["head_ref"]
        .as_str()
        .ok_or_else(|| internal("homebrew_merge_group_head_ref"))?;
    let event_repository = repository["full_name"]
        .as_str()
        .ok_or_else(|| internal("homebrew_merge_group_repository"))?;
    let event_repository_id = repository["id"]
        .as_u64()
        .ok_or_else(|| internal("homebrew_merge_group_repository"))?;
    let head_branch = valid_head_ref(head_ref)?;
    let run_id = positive_u64(runner.run_id.as_deref(), "homebrew_merge_group_run_id")?;
    let run_attempt = positive_u32(
        runner.run_attempt.as_deref(),
        "homebrew_merge_group_run_attempt",
    )?;
    if event["action"].as_str() != Some("checks_requested")
        || event_repository != expected_repository
        || event_repository_id != repository_id
        || base_ref != "refs/heads/main"
        || !lowercase_sha(base_sha)
        || !lowercase_sha(head_sha)
        || head_sha != expected_head
        || runner.git_ref.as_deref() != Some(head_ref)
    {
        return Err(internal("homebrew_merge_group_payload_mismatch"));
    }
    Ok(MergeGroupSelection::new(
        crate::package_release_publish::MergeGroupFacts {
            repository: expected_repository.to_owned(),
            repository_id,
            base_sha: base_sha.to_owned(),
            head_sha: head_sha.to_owned(),
            head_ref: head_ref.to_owned(),
            head_branch,
            run_id,
            run_attempt,
        },
    ))
}

fn valid_head_ref(value: &str) -> Result<String, OrchestratorError> {
    let branch = value
        .strip_prefix("refs/heads/")
        .filter(|branch| !branch.is_empty())
        .ok_or_else(|| internal("homebrew_merge_group_head_ref"))?;
    VcsInputs {
        commit: None,
        reference: Some(value.to_owned()),
        submodules: BTreeMap::default(),
    }
    .validate()
    .map_err(|_| internal("homebrew_merge_group_head_ref"))?;
    Ok(branch.to_owned())
}

fn positive_u64(value: Option<&str>, error: &'static str) -> Result<u64, OrchestratorError> {
    value
        .and_then(|text| text.parse::<u64>().ok())
        .filter(|number| *number > 0)
        .ok_or_else(|| internal(error))
}

fn positive_u32(value: Option<&str>, error: &'static str) -> Result<u32, OrchestratorError> {
    value
        .and_then(|text| text.parse::<u32>().ok())
        .filter(|number| *number > 0)
        .ok_or_else(|| internal(error))
}

fn lowercase_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
