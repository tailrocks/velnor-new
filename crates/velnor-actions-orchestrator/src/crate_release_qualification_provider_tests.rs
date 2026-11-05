// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::github_review_feedback::FeedbackQuery;
use crate::tap_merge::review_completion::provider::{ProviderApp, ProviderQuery};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};

const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Fixture {
    candidate: ReviewedSourceRequest,
    body: RefCell<String>,
    issue_reads: Cell<usize>,
    drift: Cell<bool>,
    bad_app: Cell<bool>,
    bad_editor: Cell<bool>,
    unknown_comment: Cell<bool>,
    deleted: Cell<bool>,
    deleted_after_first: Cell<bool>,
    timeline_reads: Cell<usize>,
    deletion_cycle: Cell<bool>,
    wrong_repository: Cell<bool>,
    missing_cursor: Cell<bool>,
    stale: Cell<bool>,
    thread: Cell<Option<bool>>,
}

impl Fixture {
    fn new() -> Self {
        let body = format!(
            "<!-- codex-pull-request-review-summary -->\n<!-- codex-review-state:v1 {{\"code_review\":{{\"commit_sha\":\"{HEAD}\",\"has_any_findings\":false,\"has_unresolved_findings\":false,\"status\":\"completed\"}},\"security_review\":{{\"commit_sha\":\"{HEAD}\",\"has_any_findings\":false,\"has_unresolved_findings\":false,\"status\":\"completed\"}}}} -->\n| 📝 **Code Review** | ✅ **Completed** <relative-time datetime=\"2026-11-05T00:00:00Z\">time</relative-time> |\n| 🔒 **Security Review** | ✅ **Completed** <relative-time datetime=\"2026-11-05T00:00:01Z\">time</relative-time> |\n"
        );
        Self {
            candidate: ReviewedSourceRequest {
                repository: "acme/example".into(),
                repository_id: 12,
                pull_request: 7,
                head_sha: HEAD.into(),
                default_branch: "main".into(),
                required_workflow_path: ".github/workflows/ci.yml".into(),
                state: ReviewedPullState::OpenHead,
            },
            body: RefCell::new(body),
            issue_reads: Cell::new(0),
            drift: Cell::new(false),
            bad_app: Cell::new(false),
            bad_editor: Cell::new(false),
            unknown_comment: Cell::new(false),
            deleted: Cell::new(false),
            deleted_after_first: Cell::new(false),
            timeline_reads: Cell::new(0),
            deletion_cycle: Cell::new(false),
            wrong_repository: Cell::new(false),
            missing_cursor: Cell::new(false),
            stale: Cell::new(false),
            thread: Cell::new(None),
        }
    }
    fn summary(&self) -> Value {
        json!({"id":5,"node_id":"IC_5","user":{"id":199_175_422,"type":"Bot"},"created_at":"2026-11-05T00:00:02Z","updated_at":"2026-11-05T00:00:02Z","body":self.body.borrow().as_str(),"performed_via_github_app":{"id":1_144_995,"slug":"chatgpt-codex-connector","owner":{"id":14_957_082}}})
    }
    fn verify(&self) -> Result<DirectFeedbackEvidence, OrchestratorError> {
        verify_direct_codex_feedback(&self.candidate, self)
    }
}

impl GitHubAdmissionRead for Fixture {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        let repo = "repos/acme/example";
        if route == repo {
            return Ok(json!({"id":12,"full_name":"acme/example","default_branch":"main"}));
        }
        if route == format!("{repo}/pulls/7") {
            return Ok(
                json!({"number":7,"commits":1,"head":{"sha":HEAD,"repo":{"id":12,"full_name":"acme/example"}},"base":{"ref":"main","repo":{"id":12,"full_name":"acme/example"}},"state":"open","draft":false,"merged":false,"mergeable_state":"clean"}),
            );
        }
        if route == format!("{repo}/rules/branches/main?per_page=100&page=1") {
            return Ok(
                json!([{"type":"pull_request","parameters":{"required_review_thread_resolution":true}},{"type":"required_status_checks","parameters":{"strict_required_status_checks_policy":true,"required_status_checks":[{"context":"Required"}]}}]),
            );
        }
        if route == format!("{repo}/commits/{HEAD}/check-runs?filter=all&per_page=100&page=1") {
            return Ok(
                json!({"total_count":1,"check_runs":[{"id":100,"name":"Required","head_sha":HEAD,"app":{"id":15368,"slug":"github-actions"},"check_suite":{"id":100},"status":"completed","conclusion":"success","started_at":"2026-11-05T00:00:00Z"}]}),
            );
        }
        if route == format!("{repo}/actions/runs?head_sha={HEAD}&per_page=100&page=1") {
            return Ok(
                json!({"total_count":1,"workflow_runs":[{"check_suite_id":100,"head_sha":HEAD,"path":".github/workflows/ci.yml","status":"completed","conclusion":"success"}]}),
            );
        }
        Err(internal("fixture_route"))
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        self.get(route).map(Some)
    }
}

impl FeedbackRead for Fixture {
    fn feedback_get(&self, route: &str) -> Result<Value, OrchestratorError> {
        let repo = "repos/acme/example";
        if route == format!("{repo}/pulls/7") {
            return self.get(route);
        }
        if route == format!("{repo}/pulls/7/commits?per_page=100&page=1") {
            return Ok(json!([{"sha":HEAD}]));
        }
        if route == format!("{repo}/issues/7/comments?per_page=100&page=1") {
            let n = self.issue_reads.get();
            self.issue_reads.set(n + 1);
            if self.drift.get() && n == 1 {
                self.body.borrow_mut().push('\n');
            }
            let mut summary = self.summary();
            if self.stale.get() {
                summary["body"] = summary["body"]
                    .as_str()
                    .unwrap_or("")
                    .replace(
                        &format!("\"security_review\":{{\"commit_sha\":\"{HEAD}\""),
                        &format!(
                            "\"security_review\":{{\"commit_sha\":\"{}\"",
                            "b".repeat(40)
                        ),
                    )
                    .into();
                *self.body.borrow_mut() = summary["body"].as_str().unwrap_or("").into();
            }
            if self.unknown_comment.get() {
                return Ok(
                    json!([summary, {"id":6,"body":"Unaddressed feedback","user":{"id":88,"type":"User"}}]),
                );
            }
            return Ok(json!([summary]));
        }
        if route == format!("{repo}/pulls/7/reviews?per_page=100&page=1")
            || route == format!("{repo}/pulls/7/comments?per_page=100&page=1")
            || route == format!("{repo}/commits/{HEAD}/comments?per_page=100&page=1")
        {
            return Ok(json!([]));
        }
        Err(internal("fixture_feedback_route"))
    }
    fn feedback_graphql(
        &self,
        query: FeedbackQuery,
        _: &[(&str, String)],
    ) -> Result<Value, OrchestratorError> {
        let nodes = self.thread.get().map_or_else(
            || json!([]),
            |resolved| json!([{"id":"thread","isResolved":resolved,"isOutdated":true}]),
        );
        Ok(match query {
            FeedbackQuery::Threads => {
                json!({"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":nodes,"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}})
            }
            FeedbackQuery::Opinions => {
                json!({"data":{"repository":{"pullRequest":{"latestOpinionatedReviews":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}})
            }
        })
    }
}

impl ProviderRead for Fixture {
    fn provider_app(&self, _: ProviderApp) -> Result<Value, OrchestratorError> {
        Ok(
            json!({"id":if self.bad_app.get() { 1 } else { 1_144_995 },"slug":"chatgpt-codex-connector","owner":{"id":14_957_082}}),
        )
    }
    fn provider_graphql(
        &self,
        query: ProviderQuery,
        _: &[(&str, String)],
    ) -> Result<Value, OrchestratorError> {
        match query {
            ProviderQuery::DeletedComments => {
                let reads = self.timeline_reads.get();
                self.timeline_reads.set(reads + 1);
                let nodes = if self.deleted.get()
                    || self.deletion_cycle.get()
                    || (self.deleted_after_first.get() && reads > 0)
                {
                    json!([{"id":format!("deleted-{reads}"),"createdAt":"2026-11-05T00:00:01Z"}])
                } else {
                    json!([])
                };
                let mut response = json!({"data":{"repository":{"fullDatabaseId":if self.wrong_repository.get() {"13"} else {"12"},"nameWithOwner":"acme/example","pullRequest":{"number":7,"timelineItems":{"nodes":nodes,"pageInfo":{"hasNextPage":self.deletion_cycle.get(),"endCursor":if self.deletion_cycle.get() {Some("same")} else {None}}}}}}});
                if self.missing_cursor.get() {
                    response
                        .pointer_mut("/data/repository/pullRequest/timelineItems/pageInfo")
                        .and_then(Value::as_object_mut)
                        .ok_or_else(|| internal("fixture_page"))?
                        .remove("endCursor");
                }
                Ok(response)
            }
            ProviderQuery::IssueEditor => Ok(
                json!({"data":{"node":{"fullDatabaseId":"5","body":self.body.borrow().as_str(),"updatedAt":"2026-11-05T00:00:02Z","lastEditedAt":null,"author":{"__typename":"Bot","databaseId":if self.bad_editor.get() { 1 } else { 199_175_422 }},"editor":null}}}),
            ),
            ProviderQuery::ThreadComments => Ok(
                json!({"data":{"node":{"id":"thread","isResolved":true,"isOutdated":true,"pullRequest":{"number":7,"repository":{"nameWithOwner":"acme/example"}},"comments":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}),
            ),
            _ => Err(internal("fixture_provider_query")),
        }
    }
    fn provider_check_runs(&self, _: &str, _: &str) -> Result<Vec<Value>, OrchestratorError> {
        Err(internal("unexpected_sonar"))
    }
}

#[test]
fn direct_provider_requires_exact_head_roles_authenticated_identity_and_stable_feedback() {
    assert!(Fixture::new().verify().is_ok());
    let f = Fixture::new();
    f.stale.set(true);
    assert!(f.verify().is_err());
    let f = Fixture::new();
    f.bad_app.set(true);
    assert!(f.verify().is_err());
    let f = Fixture::new();
    f.bad_editor.set(true);
    assert!(f.verify().is_err());
    let f = Fixture::new();
    f.unknown_comment.set(true);
    assert!(f.verify().is_err());
    let f = Fixture::new();
    f.deleted.set(true);
    assert!(f.verify().is_err());
    let f = Fixture::new();
    f.deletion_cycle.set(true);
    assert!(f.verify().is_err());
    assert_eq!(f.timeline_reads.get(), 2);
    let f = Fixture::new();
    f.deleted_after_first.set(true);
    assert!(f.verify().is_err());
    let f = Fixture::new();
    f.wrong_repository.set(true);
    assert!(f.verify().is_err());
    let f = Fixture::new();
    f.missing_cursor.set(true);
    assert!(f.verify().is_err());
    let f = Fixture::new();
    f.drift.set(true);
    assert!(f.verify().is_err());
    let f = Fixture::new();
    f.thread.set(Some(false));
    assert!(f.verify().is_err());
    let f = Fixture::new();
    f.thread.set(Some(true));
    let subject = FeedbackSubject {
        repository: "acme/example",
        pull_request_number: 7,
        head_sha: HEAD,
    };
    assert_eq!(
        crate::github_review_feedback::capture_feedback_subject(&f, &subject)
            .expect("feedback")
            .snapshot
            .unresolved_threads,
        0
    );
    assert!(f.verify().is_err());
}
