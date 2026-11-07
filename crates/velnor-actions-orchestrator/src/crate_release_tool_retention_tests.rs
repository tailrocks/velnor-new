// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
#[test]
fn release_retention_accepts_complete_paginated_artifacts() -> Result<(), Box<dyn std::error::Error>>
{
    let (request, _, now) = fixture();
    for total in [100_usize, 201] {
        let api = paginated_fixture(total);
        verify_preflight_retention(&request, &api, 10, 11, now)?;
    }
    Ok(())
}
fn paginated_fixture(total: usize) -> Api {
    let (_, mut api, _) = fixture();
    let receipt = api.0["repos/owner/project/actions/artifacts/10"].clone();
    let rust = api.0["repos/owner/project/actions/artifacts/11"].clone();
    let mut artifacts: Vec<Value> = (0..total)
        .map(|i| json!({"id": 1000 + i, "name": format!("unrelated-{i}")}))
        .collect();
    artifacts[0] = receipt;
    artifacts[total - 1] = rust;
    for (page, chunk) in artifacts.chunks(100).enumerate() {
        let body = json!({"total_count":total,"artifacts":chunk});
        let route = format!(
            "repos/owner/project/actions/runs/12/artifacts?per_page=100&page={}",
            page + 1
        );
        api.0.insert(route, body.clone());
        if page == 0 {
            api.0.insert(
                "repos/owner/project/actions/runs/12/artifacts?per_page=100".into(),
                body,
            );
        }
    }
    api
}

struct PaginationDrift {
    api: Api,
    passes: std::cell::Cell<usize>,
}
impl GitHubAdmissionRead for PaginationDrift {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        let mut response = self.api.get(route)?;
        if route == "repos/owner/project/actions/runs/12/artifacts?per_page=100" {
            self.passes.set(self.passes.get() + 1);
        } else if route.ends_with("artifacts?per_page=100&page=2") && self.passes.get() > 1 {
            response["artifacts"][0]["id"] = json!(9999);
        }
        Ok(response)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        self.api.get_optional(route)
    }
}

#[test]
fn release_retention_rejects_cross_pass_artifact_drift() {
    let (request, _, now) = fixture();
    let api = PaginationDrift {
        api: paginated_fixture(201),
        passes: std::cell::Cell::new(0),
    };
    assert!(verify_preflight_retention(&request, &api, 10, 11, now).is_err());
}

struct SlowPagination {
    api: Api,
    now: std::rc::Rc<std::cell::Cell<std::time::Instant>>,
}
impl GitHubAdmissionRead for SlowPagination {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        if route.contains("/artifacts?") {
            self.now
                .set(self.now.get() + std::time::Duration::from_secs(181));
        }
        self.api.get(route)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        self.api.get_optional(route)
    }
}

#[test]
fn release_retention_bounds_paginated_input_and_time() -> Result<(), Box<dyn std::error::Error>> {
    let (request, _, now) = fixture();
    let mut oversized = paginated_fixture(501);
    for page in 1..=5 {
        let route = if page == 1 {
            "repos/owner/project/actions/runs/12/artifacts?per_page=100".to_owned()
        } else {
            format!("repos/owner/project/actions/runs/12/artifacts?per_page=100&page={page}")
        };
        oversized.0.get_mut(&route).ok_or("page")?["artifacts"][1]["padding"] =
            json!("x".repeat(7 * 1024 * 1024));
    }
    assert!(verify_preflight_retention(&request, &oversized, 10, 11, now).is_err());
    let clock = std::rc::Rc::new(std::cell::Cell::new(std::time::Instant::now()));
    let api = SlowPagination {
        api: paginated_fixture(201),
        now: std::rc::Rc::clone(&clock),
    };
    let clock_reader = std::rc::Rc::clone(&clock);
    let result = velnor_actions_mise::CheckDeadline::with_clock(
        move || clock_reader.get(),
        || verify_preflight_retention(&request, &api, 10, 11, now),
    );
    assert!(result.is_err());
    Ok(())
}

#[test]
fn release_retention_rejects_incomplete_or_ambiguous_pages()
-> Result<(), Box<dyn std::error::Error>> {
    let (request, _, now) = fixture();
    let page2 = "repos/owner/project/actions/runs/12/artifacts?per_page=100&page=2";
    for (field, value) in [
        ("id", json!(10)),
        ("name", json!("velnor-release-receipt-12-1")),
    ] {
        let mut api = paginated_fixture(201);
        api.0.get_mut(page2).ok_or("page")?["artifacts"][0][field] = value;
        assert!(verify_preflight_retention(&request, &api, 10, 11, now).is_err());
    }
    let mut changed = paginated_fixture(201);
    changed.0.get_mut(page2).ok_or("page")?["total_count"] = json!(202);
    assert!(verify_preflight_retention(&request, &changed, 10, 11, now).is_err());
    let mut missing = paginated_fixture(201);
    missing.0.remove(page2);
    assert!(verify_preflight_retention(&request, &missing, 10, 11, now).is_err());
    let mut short = paginated_fixture(201);
    short.0.get_mut(page2).ok_or("page")?["artifacts"]
        .as_array_mut()
        .ok_or("items")?
        .pop();
    assert!(verify_preflight_retention(&request, &short, 10, 11, now).is_err());
    Ok(())
}
