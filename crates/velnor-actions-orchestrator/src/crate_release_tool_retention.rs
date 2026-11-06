// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    OrchestratorError, internal, validate_receipt_artifact_metadata,
    validate_tool_artifact_metadata,
};
use crate::crate_release_github::{GitHubAdmissionRead, GitHubAdmissionRequest};

const MAX_WORKFLOW_SECONDS: i64 = 35 * 24 * 60 * 60;

pub(crate) fn verify_preflight_retention(
    request: &GitHubAdmissionRequest,
    api: &dyn GitHubAdmissionRead,
    receipt_id: u64,
    rust_id: u64,
    now: i64,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_release_artifact_retention_invalid");
    if request.run_attempt != 1 || receipt_id == 0 || rust_id == 0 || receipt_id == rust_id {
        return Err(invalid());
    }
    let prefix = format!("repos/{}", request.repository);
    let repository = api.get(&prefix)?;
    let repository_id = repository["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(invalid)?;
    if repository["full_name"].as_str() != Some(request.repository.as_str()) {
        return Err(invalid());
    }
    let run_route = format!("{prefix}/actions/runs/{}", request.run_id);
    let run = api.get(&run_route)?;
    if run["id"].as_u64() != Some(request.run_id)
        || run["run_attempt"].as_u64() != Some(1)
        || run["head_sha"].as_str() != Some(request.workflow_sha.as_str())
        || run["repository"]["id"].as_u64() != Some(repository_id)
        || run["head_repository"]["id"].as_u64() != Some(repository_id)
    {
        return Err(invalid());
    }
    let created = timestamp(run["created_at"].as_str().ok_or_else(invalid)?).ok_or_else(invalid)?;
    let started =
        timestamp(run["run_started_at"].as_str().ok_or_else(invalid)?).ok_or_else(invalid)?;
    if created > started || started > now {
        return Err(invalid());
    }
    let limit = started
        .checked_add(MAX_WORKFLOW_SECONDS)
        .ok_or_else(invalid)?;
    let list =
        crate::release::crate_release_artifact_pages::list_artifacts(api, &prefix, request.run_id)?;
    for (id, receipt) in [(receipt_id, true), (rust_id, false)] {
        let detail = api.get(&format!("{prefix}/actions/artifacts/{id}"))?;
        let validate = if receipt {
            validate_receipt_artifact_metadata
        } else {
            validate_tool_artifact_metadata
        };
        validate(
            &list,
            &detail,
            id,
            request.run_id,
            request.run_attempt,
            repository_id,
            &request.workflow_sha,
        )?;
        let entry = list["artifacts"]
            .as_array()
            .ok_or_else(invalid)?
            .iter()
            .find(|item| item["id"].as_u64() == Some(id))
            .ok_or_else(invalid)?;
        if entry["created_at"] != detail["created_at"]
            || entry["expires_at"] != detail["expires_at"]
        {
            return Err(invalid());
        }
        let created =
            timestamp(detail["created_at"].as_str().ok_or_else(invalid)?).ok_or_else(invalid)?;
        let expires =
            timestamp(detail["expires_at"].as_str().ok_or_else(invalid)?).ok_or_else(invalid)?;
        if created < started || created > now || expires < limit || expires <= now {
            return Err(invalid());
        }
    }
    let latest_run = api.get(&run_route)?;
    let latest_repository = api.get(&prefix)?;
    if !same_run_identity(&run, &latest_run)
        || latest_repository["id"] != repository["id"]
        || latest_repository["full_name"] != repository["full_name"]
    {
        return Err(invalid());
    }
    Ok(())
}

fn same_run_identity(first: &serde_json::Value, second: &serde_json::Value) -> bool {
    [
        "id",
        "run_attempt",
        "head_sha",
        "created_at",
        "run_started_at",
    ]
    .into_iter()
    .all(|field| first[field] == second[field])
        && ["repository", "head_repository"]
            .into_iter()
            .all(|field| first[field]["id"] == second[field]["id"])
}

pub(crate) fn verify_preflight_retention_now(
    request: &GitHubAdmissionRequest,
    api: &dyn GitHubAdmissionRead,
    receipt_id: u64,
    rust_id: u64,
) -> Result<(), OrchestratorError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| internal("crate_release_retention_clock"))?
        .as_secs();
    let now = i64::try_from(now).map_err(|_| internal("crate_release_retention_clock"))?;
    verify_preflight_retention(request, api, receipt_id, rust_id, now)
}

fn timestamp(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
        || bytes
            .iter()
            .enumerate()
            .any(|(i, b)| !matches!(i, 4 | 7 | 10 | 13 | 16 | 19) && !b.is_ascii_digit())
    {
        return None;
    }
    let days = velnor_actions_contract::policy::days_between("1970-01-01", &value[..10])?;
    let hour: i64 = value[11..13].parse().ok()?;
    let minute: i64 = value[14..16].parse().ok()?;
    let second: i64 = value[17..19].parse().ok()?;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    days.checked_mul(86_400)?
        .checked_add(hour * 3_600 + minute * 60 + second)
}

#[cfg(test)]
#[path = "crate_release_tool_retention_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "crate_release_tool_retention_http_tests.rs"]
mod http_tests;
