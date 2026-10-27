// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};

use super::{GitHubApi, MergeGroupSelection};

pub(crate) struct MergeGroupPullRequest {
    pub(crate) number: u64,
    pub(crate) base_sha: String,
    pub(crate) base_ref: String,
    pub(crate) base_repository: String,
    pub(crate) base_repository_id: u64,
    pub(crate) head_sha: String,
    pub(crate) head_ref: String,
    pub(crate) head_repository: String,
    pub(crate) head_repository_id: u64,
}

pub(crate) fn read_responses(
    selection: &MergeGroupSelection,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<(Vec<u8>, Vec<u8>), OrchestratorError> {
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let api = GitHubApi::new(selection.repository().to_owned()).with_deadline(deadline);
    api.verify_preview_repository_identity(selection.repository_id(), "main")?;
    let (owner, name) = selection
        .repository()
        .split_once('/')
        .ok_or_else(|| internal("package_publish_repository_identity_invalid"))?;
    let queue_response = api.graphql_bytes(
        r#"query($owner:String!,$name:String!){repository(owner:$owner,name:$name){databaseId nameWithOwner mergeQueue(branch:"main"){entries(first:100){nodes{baseCommit{oid} headCommit{oid} pullRequest{number baseRefOid baseRefName headRefOid headRefName baseRepository{databaseId nameWithOwner} headRepository{databaseId nameWithOwner}}} pageInfo{hasNextPage}}}}}"#,
        &[("owner", owner.to_owned()), ("name", name.to_owned())],
    )?;
    let pull_request = parse_response(&queue_response, selection)?;
    let pull_response = super::read_public_merge_group_pull_request_response(
        selection.repository(),
        selection.repository_id(),
        pull_request.number,
        deadline,
    )?;
    Ok((queue_response, pull_response))
}

pub(crate) fn parse_response(
    bytes: &[u8],
    selection: &MergeGroupSelection,
) -> Result<MergeGroupPullRequest, OrchestratorError> {
    let response: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| internal("package_publish_api_json"))?;
    if response
        .get("errors")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|errors| !errors.is_empty())
    {
        return Err(internal("package_publish_api_graphql_errors"));
    }
    let repository = response
        .pointer("/data/repository")
        .filter(|repository| {
            repository
                .get("nameWithOwner")
                .and_then(serde_json::Value::as_str)
                == Some(selection.repository())
                && repository
                    .get("databaseId")
                    .and_then(serde_json::Value::as_u64)
                    == Some(selection.repository_id())
        })
        .ok_or_else(|| internal("package_public_merge_group_repository"))?;
    let entries = repository
        .pointer("/mergeQueue/entries")
        .ok_or_else(|| internal("package_public_merge_group_entries"))?;
    if entries
        .pointer("/pageInfo/hasNextPage")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        return Err(internal("package_public_merge_group_entries_truncated"));
    }
    let nodes = entries
        .get("nodes")
        .and_then(serde_json::Value::as_array)
        .filter(|nodes| nodes.len() <= 100)
        .ok_or_else(|| internal("package_public_merge_group_entries"))?;
    let matches = nodes
        .iter()
        .filter(|entry| {
            entry
                .pointer("/baseCommit/oid")
                .and_then(serde_json::Value::as_str)
                == Some(selection.base_sha())
                && entry
                    .pointer("/headCommit/oid")
                    .and_then(serde_json::Value::as_str)
                    == Some(selection.head_sha())
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(internal("package_public_merge_group_entry_ambiguous"));
    }
    let pull = matches[0]
        .get("pullRequest")
        .filter(|pull| !pull.is_null())
        .ok_or_else(|| internal("package_public_merge_group_pull_request"))?;
    let base_repository = parse_repository(pull, "baseRepository")?;
    let head_repository = parse_repository(pull, "headRepository")?;
    Ok(MergeGroupPullRequest {
        number: pull
            .get("number")
            .and_then(serde_json::Value::as_u64)
            .filter(|number| *number > 0)
            .ok_or_else(|| internal("package_public_merge_group_pull_request"))?,
        base_sha: parse_sha(pull, "baseRefOid")?,
        base_ref: parse_ref(pull, "baseRefName")?,
        base_repository: base_repository.1,
        base_repository_id: base_repository.0,
        head_sha: parse_sha(pull, "headRefOid")?,
        head_ref: parse_ref(pull, "headRefName")?,
        head_repository: head_repository.1,
        head_repository_id: head_repository.0,
    })
}

fn parse_repository(
    pull: &serde_json::Value,
    field: &str,
) -> Result<(u64, String), OrchestratorError> {
    let repository = pull
        .get(field)
        .filter(|repository| !repository.is_null())
        .ok_or_else(|| internal("package_public_merge_group_repository"))?;
    let id = repository
        .get("databaseId")
        .and_then(serde_json::Value::as_u64)
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("package_public_merge_group_repository"))?;
    let name = repository
        .get("nameWithOwner")
        .and_then(serde_json::Value::as_str)
        .filter(|name| velnor_actions_contract::vcs::is_safe_github_repository(name))
        .ok_or_else(|| internal("package_public_merge_group_repository"))?;
    Ok((id, name.to_owned()))
}

fn parse_sha(pull: &serde_json::Value, field: &str) -> Result<String, OrchestratorError> {
    let value = pull
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| super::github::is_lowercase_git_sha(value))
        .ok_or_else(|| internal("package_public_merge_group_pull_request"))?;
    Ok(value.to_owned())
}

fn parse_ref(pull: &serde_json::Value, field: &str) -> Result<String, OrchestratorError> {
    let value = pull
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 255
                && !value.bytes().any(|byte| byte.is_ascii_control())
        })
        .ok_or_else(|| internal("package_public_merge_group_pull_request"))?;
    Ok(value.to_owned())
}
