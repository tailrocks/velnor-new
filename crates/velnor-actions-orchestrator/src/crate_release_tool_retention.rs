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
    let list = api.get(&format!(
        "{prefix}/actions/runs/{}/artifacts?per_page=100",
        request.run_id
    ))?;
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
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;

    struct Api(BTreeMap<String, Value>);
    impl GitHubAdmissionRead for Api {
        fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
            self.0
                .get(route)
                .cloned()
                .ok_or_else(|| internal("fixture_missing"))
        }
        fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
            Ok(self.0.get(route).cloned())
        }
    }
    struct ChangingApi {
        api: Api,
        reads: std::cell::Cell<usize>,
        field: &'static str,
    }
    impl GitHubAdmissionRead for ChangingApi {
        fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
            let mut value = self.api.get(route)?;
            if route == "repos/owner/project/actions/runs/12" {
                let count = self.reads.get();
                self.reads.set(count + 1);
                if count > 0 {
                    value[self.field] = json!("changed");
                }
            }
            Ok(value)
        }
        fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
            self.api.get_optional(route)
        }
    }
    fn fixture() -> (GitHubAdmissionRequest, Api, i64) {
        let sha = "a".repeat(40);
        let request = GitHubAdmissionRequest {
            repository: "owner/project".into(),
            default_branch: "main".into(),
            environment: "release".into(),
            trusted_reviewer_id: 1,
            workflow_path: ".github/workflows/release.yml".into(),
            required_workflow_path: ".github/workflows/ci.yml".into(),
            workflow_sha: sha.clone(),
            run_id: 12,
            run_attempt: 1,
            event: "push".into(),
            source_sha: sha.clone(),
            recovery_sha: None,
        };
        let make = |id, purpose| json!({"id": id, "name": format!("velnor-release-{purpose}-12-1"), "digest": format!("sha256:{}", "c".repeat(64)), "size_in_bytes": 100, "expired": false, "created_at": "2026-01-01T00:00:10Z", "expires_at": "2026-02-05T00:00:00Z", "workflow_run": {"id":12,"repository_id":8,"head_repository_id":8,"head_sha":sha}});
        let receipt = make(10, "receipt");
        let rust = make(11, "rust");
        let api = Api(BTreeMap::from([
            (
                "repos/owner/project".into(),
                json!({"id":8,"full_name":"owner/project"}),
            ),
            (
                "repos/owner/project/actions/runs/12".into(),
                json!({"id":12,"run_attempt":1,"head_sha":sha,"repository":{"id":8},"head_repository":{"id":8},"created_at":"2026-01-01T00:00:00Z","run_started_at":"2026-01-01T00:00:00Z"}),
            ),
            (
                "repos/owner/project/actions/runs/12/artifacts?per_page=100".into(),
                json!({"total_count":2,"artifacts":[receipt.clone(),rust.clone()]}),
            ),
            ("repos/owner/project/actions/artifacts/10".into(), receipt),
            ("repos/owner/project/actions/artifacts/11".into(), rust),
        ]));
        (
            request,
            api,
            timestamp("2026-01-01T01:00:00Z").expect("fixture time"),
        )
    }
    #[test]
    fn effective_preflight_retention_is_bound_to_run_and_both_artifacts()
    -> Result<(), Box<dyn std::error::Error>> {
        let (request, api, now) = fixture();
        verify_preflight_retention(&request, &api, 10, 11, now)?;
        for (field, value) in [
            ("expires_at", json!("2026-02-04T23:59:59Z")),
            ("expires_at", json!("invalid")),
            ("created_at", json!("2026-01-02T00:00:00Z")),
            ("expired", json!(true)),
            ("id", json!(99)),
        ] {
            let mut changed = fixture().1;
            changed
                .0
                .get_mut("repos/owner/project/actions/artifacts/10")
                .ok_or("detail")?[field] = value.clone();
            changed
                .0
                .get_mut("repos/owner/project/actions/runs/12/artifacts?per_page=100")
                .ok_or("list")?["artifacts"][0][field] = value;
            assert!(
                verify_preflight_retention(&request, &changed, 10, 11, now).is_err(),
                "{field}"
            );
        }
        for (field, value) in [
            ("id", json!(13)),
            ("run_attempt", json!(2)),
            ("head_sha", json!("b".repeat(40))),
            ("repository", json!({"id": 9})),
            ("head_repository", json!({"id": 9})),
            ("created_at", json!(null)),
            ("run_started_at", json!("9999-99-99T00:00:00Z")),
        ] {
            let mut changed = fixture().1;
            changed
                .0
                .get_mut("repos/owner/project/actions/runs/12")
                .ok_or("run")?[field] = value;
            assert!(
                verify_preflight_retention(&request, &changed, 10, 11, now).is_err(),
                "run {field}"
            );
        }
        for (field, value) in [("total_count", json!(3)), ("artifacts", json!([]))] {
            let mut changed = fixture().1;
            changed
                .0
                .get_mut("repos/owner/project/actions/runs/12/artifacts?per_page=100")
                .ok_or("list")?[field] = value;
            assert!(
                verify_preflight_retention(&request, &changed, 10, 11, now).is_err(),
                "list {field}"
            );
        }
        let mut queued = fixture().1;
        queued
            .0
            .get_mut("repos/owner/project/actions/runs/12")
            .ok_or("run")?["run_started_at"] = json!("2026-01-01T00:30:00Z");
        for (i, id) in [10, 11].into_iter().enumerate() {
            queued
                .0
                .get_mut(&format!("repos/owner/project/actions/artifacts/{id}"))
                .ok_or("detail")?["created_at"] = json!("2026-01-01T00:30:10Z");
            queued
                .0
                .get_mut("repos/owner/project/actions/runs/12/artifacts?per_page=100")
                .ok_or("list")?["artifacts"][i]["created_at"] = json!("2026-01-01T00:30:10Z");
        }
        assert!(verify_preflight_retention(&request, &queued, 10, 11, now).is_err());
        let mut rerun = request.clone();
        rerun.run_attempt = 2;
        assert!(verify_preflight_retention(&rerun, &api, 10, 11, now).is_err());
        assert!(verify_preflight_retention(&request, &api, 10, 10, now).is_err());
        let mut altered = fixture().1;
        altered
            .0
            .get_mut("repos/owner/project/actions/artifacts/11")
            .ok_or("rust")?["expires_at"] = json!("2026-02-04T23:59:59Z");
        assert!(verify_preflight_retention(&request, &altered, 10, 11, now).is_err());
        Ok(())
    }

    #[test]
    fn retention_rejects_identity_and_reread_drift() -> Result<(), Box<dyn std::error::Error>> {
        let (request, api, now) = fixture();
        for (field, value) in [
            ("name", json!("other")),
            ("id", json!(0)),
            (
                "workflow_run",
                json!({"id": 99, "repository_id": 8, "head_repository_id": 8, "head_sha": "a".repeat(40)}),
            ),
        ] {
            let mut changed = fixture().1;
            changed
                .0
                .get_mut("repos/owner/project/actions/artifacts/10")
                .ok_or("detail")?[field] = value.clone();
            changed
                .0
                .get_mut("repos/owner/project/actions/runs/12/artifacts?per_page=100")
                .ok_or("list")?["artifacts"][0][field] = value;
            assert!(verify_preflight_retention(&request, &changed, 10, 11, now).is_err());
        }
        let changed = ChangingApi {
            api: fixture().1,
            reads: std::cell::Cell::new(0),
            field: "head_sha",
        };
        assert!(verify_preflight_retention(&request, &changed, 10, 11, now).is_err());
        let benign = ChangingApi {
            api,
            reads: std::cell::Cell::new(0),
            field: "updated_at",
        };
        verify_preflight_retention(&request, &benign, 10, 11, now)?;
        Ok(())
    }
}
