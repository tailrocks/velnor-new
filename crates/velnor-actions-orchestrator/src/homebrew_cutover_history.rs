// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::github::TapMergeGitHub;
use crate::{
    OrchestratorError,
    github_landed_pull::{RestLandedPull, RestPullRef, read_merge_oid},
    internal::internal,
};
use serde_json::Value;

const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::tap_merge) struct ObservedCutoverPull {
    pub(in crate::tap_merge) number: u64,
    pub(in crate::tap_merge) head: String,
    pub(in crate::tap_merge) base: String,
    pub(in crate::tap_merge) landed: String,
    pub(in crate::tap_merge) merged_at: String,
}

pub(super) fn find(
    api: &TapMergeGitHub,
    repository: &str,
    repository_id: u64,
    current_main: &str,
) -> Result<ObservedCutoverPull, OrchestratorError> {
    let path =
        super::super::observation::encode_segment(crate::homebrew::structural_cutover::MARKER_PATH);
    let commits = pages(
        api,
        &format!("repos/{repository}/commits?sha={current_main}&path={path}"),
    )?;
    if commits.len() != 1 {
        return Err(internal("homebrew_cutover_marker_history_count"));
    }
    let marker_commit = sha(&commits[0], "/sha")?;
    let pulls = pages(
        api,
        &format!("repos/{repository}/commits/{marker_commit}/pulls"),
    )?;
    if pulls.len() != 1 {
        return Err(internal("homebrew_cutover_pull_history_count"));
    }
    let number = pulls[0]["number"]
        .as_u64()
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("homebrew_cutover_pull_number"))?;
    let observed = read(api, repository, repository_id, number)?;
    if observed.landed != marker_commit {
        return Err(internal("homebrew_cutover_marker_commit_mismatch"));
    }
    Ok(observed)
}

pub(super) fn read(
    api: &TapMergeGitHub,
    repository: &str,
    repository_id: u64,
    number: u64,
) -> Result<ObservedCutoverPull, OrchestratorError> {
    let detail: Value = api.get(&format!("repos/{repository}/pulls/{number}"))?;
    if detail["number"].as_u64() != Some(number)
        || detail["state"].as_str() != Some("closed")
        || detail["merged"].as_bool() != Some(true)
        || detail["draft"].as_bool() != Some(false)
        || detail.pointer("/base/ref").and_then(Value::as_str) != Some("main")
        || detail
            .pointer("/user/id")
            .and_then(Value::as_u64)
            .is_none_or(|id| id == 0)
    {
        return Err(internal("homebrew_cutover_pull_identity"));
    }
    let merged_at = text(&detail, "/merged_at")?;
    if !super::timestamp::valid_timestamp(merged_at) {
        return Err(internal("homebrew_cutover_pull_merge_time"));
    }
    let rest = RestLandedPull {
        number,
        state: "closed",
        merged: true,
        merged_at,
        head: pull_ref(&detail, "/head")?,
        base: pull_ref(&detail, "/base")?,
        merge_sha: optional_merge_sha(&detail)?,
    };
    let landed = read_merge_oid(repository_id, repository, &rest, |query, fields| {
        api.graphql(query, fields)
    })?;
    Ok(ObservedCutoverPull {
        number,
        head: sha(&detail, "/head/sha")?,
        base: sha(&detail, "/base/sha")?,
        landed,
        merged_at: merged_at.to_owned(),
    })
}

fn pages(api: &TapMergeGitHub, route: &str) -> Result<Vec<Value>, OrchestratorError> {
    let mut result = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let separator = if route.contains('?') { '&' } else { '?' };
    for page in 1..=MAX_PAGES {
        let batch: Vec<Value> = api.get(&format!(
            "{route}{separator}per_page={PAGE_SIZE}&page={page}"
        ))?;
        if batch.len() > PAGE_SIZE {
            return Err(internal("homebrew_cutover_history_page_size"));
        }
        let length = batch.len();
        for item in batch {
            let identity = if let Some(number) = item["number"].as_u64().filter(|id| *id > 0) {
                format!("pull:{number}")
            } else {
                format!("commit:{}", sha(&item, "/sha")?)
            };
            if !seen.insert(identity) {
                return Err(internal("homebrew_cutover_history_duplicate"));
            }
            result.push(item);
        }
        if length < PAGE_SIZE {
            return Ok(result);
        }
    }
    Err(internal("homebrew_cutover_history_incomplete"))
}

fn pull_ref<'a>(detail: &'a Value, pointer: &str) -> Result<RestPullRef<'a>, OrchestratorError> {
    let value = detail
        .pointer(pointer)
        .ok_or_else(|| internal("homebrew_cutover_pull_ref"))?;
    Ok(RestPullRef {
        reference: text(value, "/ref")?,
        sha: text(value, "/sha")?,
        repository: text(value, "/repo/full_name")?,
        repository_id: value
            .pointer("/repo/id")
            .and_then(Value::as_u64)
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("homebrew_cutover_pull_ref"))?,
    })
}

fn optional_merge_sha(value: &Value) -> Result<Option<&str>, OrchestratorError> {
    match value.get("merge_commit_sha") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(sha)) if valid_sha(sha) => Ok(Some(sha)),
        _ => Err(internal("homebrew_cutover_pull_merge_sha")),
    }
}

pub(super) fn text<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, OrchestratorError> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("homebrew_cutover_history_field"))
}

pub(super) fn sha(value: &Value, pointer: &str) -> Result<String, OrchestratorError> {
    text(value, pointer)
        .ok()
        .filter(|value| valid_sha(value))
        .map(str::to_owned)
        .ok_or_else(|| internal("homebrew_cutover_history_sha"))
}

pub(super) fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
