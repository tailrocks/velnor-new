// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ReviewedPullState, ReviewedSourceRequest, verify_pull_source};
use crate::{OrchestratorError, crate_release_github::GitHubAdmissionRead, internal::internal};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};

struct Fixture {
    request: ReviewedSourceRequest,
    values: RefCell<BTreeMap<String, Value>>,
    reads: Cell<usize>,
    mutate: Cell<bool>,
}
impl Fixture {
    fn new(merged: bool) -> Self {
        let request = ReviewedSourceRequest {
            repository: "acme/example".into(),
            repository_id: 12,
            pull_request: 7,
            head_sha: "a".repeat(40),
            default_branch: "main".into(),
            required_workflow_path: ".github/workflows/ci.yml".into(),
            state: if merged {
                ReviewedPullState::Merged {
                    merge_sha: "b".repeat(40),
                    merge_tree_sha: "c".repeat(40),
                }
            } else {
                ReviewedPullState::OpenHead
            },
        };
        let repo = "repos/acme/example";
        let mut pull = json!({"number":7,"head":{"sha":request.head_sha,"repo":{"id":12,"full_name":"acme/example"}},"base":{"ref":"main","repo":{"id":12,"full_name":"acme/example"}},"state":"open","draft":false,"merged":false,"mergeable_state":"clean"});
        if merged {
            pull["state"] = "closed".into();
            pull["merged"] = true.into();
            pull["merged_at"] = "2026-11-05T00:00:00Z".into();
            pull["merge_commit_sha"] = "b".repeat(40).into();
        }
        let mut values = BTreeMap::from([
            (
                repo.into(),
                json!({"id":12,"full_name":"acme/example","default_branch":"main"}),
            ),
            (format!("{repo}/pulls/7"), pull),
            (
                format!("{repo}/rules/branches/main?per_page=100&page=1"),
                json!([{"type":"pull_request","parameters":{"required_review_thread_resolution":true}},{"type":"required_status_checks","parameters":{"strict_required_status_checks_policy":!merged,"required_status_checks":[{"context":"Required"}]}}]),
            ),
            (
                format!("{repo}/git/commits/{}", "b".repeat(40)),
                json!({"sha":"b".repeat(40),"tree":{"sha":"c".repeat(40)}}),
            ),
            (
                format!("{repo}/compare/{}...main", "b".repeat(40)),
                json!({"status":"ahead"}),
            ),
        ]);
        for (id, sha) in [(100, "a".repeat(40)), (200, "b".repeat(40))] {
            values.insert(format!("{repo}/commits/{sha}/check-runs?filter=all&per_page=100&page=1"),json!({"total_count":1,"check_runs":[{"id":id,"name":"Required","head_sha":sha,"app":{"id":15368,"slug":"github-actions"},"check_suite":{"id":id},"status":"completed","conclusion":"success","started_at":"2026-11-05T00:00:00Z"}]}));
            values.insert(format!("{repo}/actions/runs?head_sha={sha}&per_page=100&page=1"),json!({"total_count":1,"workflow_runs":[{"check_suite_id":id,"head_sha":sha,"path":".github/workflows/ci.yml","status":"completed","conclusion":"success"}]}));
        }
        Self {
            request,
            values: RefCell::new(values),
            reads: Cell::new(0),
            mutate: Cell::new(false),
        }
    }
}
impl GitHubAdmissionRead for Fixture {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        let count = self.reads.get();
        self.reads.set(count + 1);
        let mut value = self
            .values
            .borrow()
            .get(route)
            .cloned()
            .ok_or_else(|| internal("unexpected_fixture_route"))?;
        if self.mutate.get() && count >= 5 && route.ends_with("/pulls/7") {
            value["head"]["sha"] = "d".repeat(40).into();
        }
        Ok(value)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        self.get(route).map(Some)
    }
}
#[test]
fn accepts_exact_open_candidate_and_merged_source() {
    for merged in [false, true] {
        let f = Fixture::new(merged);
        assert!(verify_pull_source(&f.request, &f).is_ok());
    }
}
#[test]
fn rejects_wrong_merge_tree_and_source_not_on_default_branch() {
    let f = Fixture::new(true);
    let commit = format!("repos/acme/example/git/commits/{}", "b".repeat(40));
    f.values.borrow_mut().get_mut(&commit).expect("fixture")["tree"]["sha"] = "d".repeat(40).into();
    assert!(verify_pull_source(&f.request, &f).is_err());
    f.values.borrow_mut().get_mut(&commit).expect("fixture")["tree"]["sha"] = "c".repeat(40).into();
    let compare = format!("repos/acme/example/compare/{}...main", "b".repeat(40));
    f.values.borrow_mut().get_mut(&compare).expect("fixture")["status"] = "diverged".into();
    assert!(verify_pull_source(&f.request, &f).is_err());
}
#[test]
fn rejects_failed_check_unprotected_rules_and_changed_head() {
    let f = Fixture::new(false);
    let checks = format!(
        "repos/acme/example/commits/{}/check-runs?filter=all&per_page=100&page=1",
        "a".repeat(40)
    );
    f.values.borrow_mut().get_mut(&checks).expect("fixture")["check_runs"][0]["conclusion"] =
        "failure".into();
    assert!(verify_pull_source(&f.request, &f).is_err());
    f.values.borrow_mut().get_mut(&checks).expect("fixture")["check_runs"][0]["conclusion"] =
        "success".into();
    let rules = "repos/acme/example/rules/branches/main?per_page=100&page=1";
    f.values.borrow_mut().get_mut(rules).expect("fixture")[1]["parameters"]["strict_required_status_checks_policy"] =
        false.into();
    assert!(verify_pull_source(&f.request, &f).is_err());
    f.values.borrow_mut().get_mut(rules).expect("fixture")[1]["parameters"]["strict_required_status_checks_policy"] =
        true.into();
    f.reads.set(0);
    f.mutate.set(true);
    assert!(verify_pull_source(&f.request, &f).is_err());
}
