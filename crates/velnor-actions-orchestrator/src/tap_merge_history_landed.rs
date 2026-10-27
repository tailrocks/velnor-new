// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use super::{MergeAuthority, MergeCandidate, TapMergeGitHub, required_text};
use crate::{
    OrchestratorError,
    github_landed_pull::{RestLandedPull, RestPullRef, read_merge_oid},
    internal::internal,
};

pub(super) fn merged_fields(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    api: &TapMergeGitHub,
    number: u64,
    detail: &Value,
) -> Result<(bool, Option<String>, Option<String>), OrchestratorError> {
    let merged = detail["merged"]
        .as_bool()
        .ok_or_else(|| internal("tap_merge_history_merged_missing"))?;
    if !merged {
        return Ok((false, None, None));
    }
    if detail["state"].as_str() != Some("closed") {
        return Err(internal("tap_merge_history_merged_state"));
    }
    let merged_at = required_text(detail, "/merged_at", "tap_merge_history_merged_at")?;
    let merge_sha = match detail.get("merge_commit_sha") {
        None | Some(Value::Null) => None,
        Some(Value::String(sha)) => Some(sha.as_str()),
        Some(_) => return Err(internal("tap_merge_history_merge_sha")),
    };
    let rest = RestLandedPull {
        number,
        state: "closed",
        merged,
        merged_at,
        head: pull_ref(detail, "/head")?,
        base: pull_ref(detail, "/base")?,
        merge_sha,
    };
    let sha = read_merge_oid(
        authority.repo_id,
        &candidate.policy.repository,
        &rest,
        |query, fields| api.graphql(query, fields),
    )?;
    Ok((true, Some(sha), Some(merged_at.to_owned())))
}

fn pull_ref<'a>(detail: &'a Value, pointer: &str) -> Result<RestPullRef<'a>, OrchestratorError> {
    let value = detail
        .pointer(pointer)
        .ok_or_else(|| internal("tap_merge_history_pr_identity"))?;
    let repository_id = value
        .pointer("/repo/id")
        .and_then(Value::as_u64)
        .ok_or_else(|| internal("tap_merge_history_repository_id"))?;
    Ok(RestPullRef {
        reference: required_text(value, "/ref", "tap_merge_history_pr_identity")?,
        sha: required_text(value, "/sha", "tap_merge_history_pr_identity")?,
        repository_id,
        repository: required_text(value, "/repo/full_name", "tap_merge_history_pr_identity")?,
    })
}
