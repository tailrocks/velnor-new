// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(super) struct State {
    api: Api,
    mutation: Mutation,
    source_reads: usize,
    issue_reads: usize,
    check_reads: usize,
    counts: Arc<Counts>,
}
impl State {
    pub(super) fn new(mut api: Api, mutation: Mutation, counts: Arc<Counts>) -> Self {
        api.values
            .get_mut("repos/acme/candidate/pulls/7")
            .expect("candidate")["commits"] = 1.into();
        api.values.insert(
            "repos/acme/candidate/pulls/7/commits?per_page=100&page=1".into(),
            json!([{"sha":"a".repeat(40)}]),
        );
        for path in [
            "pulls/7/reviews",
            "pulls/7/comments",
            "commits/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/comments",
        ] {
            api.values.insert(
                format!("repos/acme/candidate/{path}?per_page=100&page=1"),
                json!([]),
            );
        }
        Self {
            api,
            mutation,
            source_reads: 0,
            issue_reads: 0,
            check_reads: 0,
            counts,
        }
    }

    pub(super) fn respond(
        &mut self,
        request: &Request,
        storage: &str,
        api_origin: &str,
    ) -> Response {
        let route = request.path.strip_prefix('/').unwrap_or(&request.path);
        if route == "graphql" {
            return self.graphql(&request.body);
        }
        if route == "apps/chatgpt-codex-connector" {
            if self.mutation == Mutation::App {
                self.trigger();
            }
            return Response::json(
                &json!({"id":if self.mutation == Mutation::App { 1 } else { 1_144_995 },"slug":"chatgpt-codex-connector","owner":{"id":14_957_082}}),
            );
        }
        if route == "repos/acme/candidate/issues/7/comments?per_page=100&page=1" {
            self.issue_reads += 1;
            let summary = self.summary();
            if self.mutation == Mutation::Duplicate {
                self.trigger();
            }
            return Response::json(&if self.mutation == Mutation::Duplicate {
                json!([summary, summary])
            } else {
                json!([summary])
            });
        }
        if route == "repos/acme/consumer/actions/jobs/44/logs" {
            let origin = if self.mutation == Mutation::Redirect {
                self.trigger();
                api_origin
            } else {
                storage
            };
            return Response::redirect(format!("{origin}/storage/log"));
        }
        if let Some(id) = route
            .strip_prefix("repos/acme/consumer/actions/artifacts/")
            .and_then(|suffix| suffix.strip_suffix("/zip"))
        {
            return Response::redirect(format!("{storage}/storage/a{id}"));
        }
        let Some(mut value) = self.api.values.get(route).cloned() else {
            return Response::status(404);
        };
        if route == "repos/acme/candidate" && self.mutation == Mutation::Repository {
            self.trigger();
            value["id"] = 13.into();
        }
        if route == "repos/acme/consumer/pulls/8" {
            self.source_reads += 1;
            if self.mutation == Mutation::Source && self.source_reads > 2 {
                self.trigger();
                value["head"]["sha"] = "f".repeat(40).into();
            }
        }
        if route.starts_with("repos/acme/candidate/commits/") && route.contains("/check-runs?") {
            self.check_reads += 1;
            if self.mutation == Mutation::Checks && self.check_reads > 2 {
                self.trigger();
                value["check_runs"][0]["conclusion"] = "failure".into();
            }
        }
        Response::json(&value)
    }

    fn body(&self) -> String {
        let head = "a".repeat(40);
        let mut text = format!(
            "<!-- codex-pull-request-review-summary -->\n<!-- codex-review-state:v1 {{\"code_review\":{{\"commit_sha\":\"{head}\",\"has_any_findings\":false,\"has_unresolved_findings\":false,\"status\":\"completed\"}},\"security_review\":{{\"commit_sha\":\"{head}\",\"has_any_findings\":false,\"has_unresolved_findings\":false,\"status\":\"completed\"}}}} -->\n| 📝 **Code Review** | ✅ **Completed** <relative-time datetime=\"2026-11-05T00:00:00Z\">time</relative-time> |\n| 🔒 **Security Review** | ✅ **Completed** <relative-time datetime=\"2026-11-05T00:00:01Z\">time</relative-time> |\n"
        );
        if self.mutation == Mutation::Feedback
            && self.issue_reads > 0
            && self.issue_reads.is_multiple_of(2)
        {
            self.trigger();
            text.push('\n');
        }
        text
    }

    fn summary(&self) -> Value {
        json!({"id":5,"node_id":"IC_5","user":{"id":199_175_422,"type":"Bot"},"created_at":"2026-11-05T00:00:02Z","updated_at":"2026-11-05T00:00:02Z","body":self.body(),"performed_via_github_app":{"id":1_144_995,"slug":"chatgpt-codex-connector","owner":{"id":14_957_082}}})
    }

    fn graphql(&self, body: &[u8]) -> Response {
        let Ok(value) = serde_json::from_slice::<Value>(body) else {
            return Response::status(400);
        };
        let query = value["query"].as_str().unwrap_or("");
        if query.contains("reviewThreads") {
            if self.mutation == Mutation::Pagination {
                self.trigger();
            }
            return Response::json(
                &json!({"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[],"pageInfo":{"hasNextPage":self.mutation == Mutation::Pagination,"endCursor":null}}}}}}),
            );
        }
        if query.contains("latestOpinionatedReviews") {
            return Response::json(
                &json!({"data":{"repository":{"pullRequest":{"latestOpinionatedReviews":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}),
            );
        }
        if query.contains("COMMENT_DELETED_EVENT") {
            return Response::json(
                &json!({"data":{"repository":{"fullDatabaseId":"11","nameWithOwner":"acme/candidate","pullRequest":{"number":7,"timelineItems":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}),
            );
        }
        if query.contains("IssueComment") {
            return Response::json(
                &json!({"data":{"node":{"fullDatabaseId":"5","body":self.body(),"updatedAt":"2026-11-05T00:00:02Z","lastEditedAt":null,"author":{"__typename":"Bot","databaseId":199_175_422},"editor":null}}}),
            );
        }
        Response::status(404)
    }

    fn trigger(&self) {
        self.counts.triggered.store(true, Ordering::SeqCst);
    }
}
