// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{GitHubAdmissionHttp, JSON_LIMIT, OrchestratorError, internal, parse_json};
use serde_json::Value;

const THREADS: &str = "query($owner:String!,$name:String!,$number:Int!,$cursor:String){repository(owner:$owner,name:$name){pullRequest(number:$number){reviewThreads(first:100,after:$cursor){nodes{id isResolved isOutdated} pageInfo{hasNextPage endCursor}}}}}";

const OPINIONS: &str = "query($owner:String!,$name:String!,$number:Int!,$cursor:String){repository(owner:$owner,name:$name){pullRequest(number:$number){latestOpinionatedReviews(first:100,after:$cursor){nodes{id state submittedAt author{__typename ... on User{databaseId} ... on Bot{databaseId}}} pageInfo{hasNextPage endCursor}}}}}";

impl GitHubAdmissionHttp {
    pub fn qualification_review_threads(
        &self,
        repository: &str,
        number: u32,
        cursor: Option<&str>,
    ) -> Result<Value, OrchestratorError> {
        let value = self.qualification_graphql_page(repository, number, cursor, THREADS)?;
        validate_page(&value, "reviewThreads")?;
        Ok(value)
    }

    pub fn qualification_opinionated_reviews(
        &self,
        repository: &str,
        number: u32,
        cursor: Option<&str>,
    ) -> Result<Value, OrchestratorError> {
        let value = self.qualification_graphql_page(repository, number, cursor, OPINIONS)?;
        validate_page(&value, "latestOpinionatedReviews")?;
        Ok(value)
    }

    fn qualification_graphql_page(
        &self,
        repository: &str,
        number: u32,
        cursor: Option<&str>,
        query: &'static str,
    ) -> Result<Value, OrchestratorError> {
        let Some((owner, name)) = repository.split_once('/') else {
            return Err(internal("qualification_repository_invalid"));
        };
        if !velnor_actions_contract::config::is_valid_github_owner(owner)
            || !velnor_actions_contract::config::is_valid_github_repo_name(name)
            || number == 0
            || cursor.is_some_and(|s| {
                s.is_empty() || s.len() > 512 || s.bytes().any(|b| !b.is_ascii_graphic())
            })
        {
            return Err(internal("qualification_graphql_input"));
        }
        let remaining = self
            .deadline
            .saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(internal("qualification_graphql_deadline"));
        }
        let body = serde_json::to_vec(&serde_json::json!({"query":query,"variables":{"owner":owner,"name":name,"number":number,"cursor":cursor}}))
            .map_err(|_| internal("qualification_graphql_request"))?;
        let response = crate::api::github_evidence_http::agent()?
            .post(format!("{}/graphql", self.origin))
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("Accept-Encoding", "identity")
            .header("Content-Type", "application/json")
            .header("User-Agent", "velnor-actions-qualification")
            .config()
            .timeout_global(Some(remaining))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .send(body.as_slice())
            .map_err(|_| internal("qualification_graphql_transport"))?;
        if response.status().as_u16() != 200 {
            return Err(internal("qualification_graphql_status"));
        }
        let value = parse_json(response, self.deadline, JSON_LIMIT)?;
        if value.get("errors").is_some() {
            return Err(internal("qualification_graphql_errors"));
        }
        Ok(value)
    }
}

fn validate_page(value: &Value, connection: &str) -> Result<(), OrchestratorError> {
    let invalid = || internal("qualification_graphql_page");
    let page = value
        .pointer(&format!("/data/repository/pullRequest/{connection}"))
        .ok_or_else(invalid)?;
    let nodes = page["nodes"]
        .as_array()
        .filter(|items| items.len() <= 100)
        .ok_or_else(invalid)?;
    if nodes.iter().any(|node| {
        node["id"]
            .as_str()
            .is_none_or(|id| id.is_empty() || id.len() > 512)
            || if connection == "reviewThreads" {
                node["isResolved"].as_bool().is_none() || node["isOutdated"].as_bool().is_none()
            } else {
                node["state"].as_str().is_none_or(str::is_empty)
                    || node["submittedAt"].as_str().is_none_or(str::is_empty)
                    || !matches!(node["author"]["__typename"].as_str(), Some("User" | "Bot"))
                    || node["author"]["databaseId"]
                        .as_u64()
                        .is_none_or(|id| id == 0)
            }
    }) {
        return Err(invalid());
    }
    let more = page["pageInfo"]["hasNextPage"]
        .as_bool()
        .ok_or_else(invalid)?;
    let cursor = &page["pageInfo"]["endCursor"];
    if more
        && cursor.as_str().is_none_or(|s| {
            s.is_empty() || s.len() > 512 || s.bytes().any(|b| !b.is_ascii_graphic())
        })
    {
        return Err(invalid());
    }
    if !(more
        || cursor.is_null()
        || cursor
            .as_str()
            .is_some_and(|s| s.len() <= 512 && s.bytes().all(|b| b.is_ascii_graphic())))
    {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::tests::{client, reply, server};

    #[test]
    fn fixed_query_reads_threads_without_accepting_mutations()
    -> Result<(), Box<dyn std::error::Error>> {
        let (origin, worker) = server(vec![reply(
            "200 OK",
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        )]);
        let api = client(origin);
        let value = api.qualification_review_threads("owner/repo", 7, None)?;
        assert!(value["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"].is_array());
        let requests = worker.join().map_err(|_| "server")?;
        assert!(requests[0].starts_with("POST /graphql "));
        assert!(requests[0].contains("reviewThreads"));
        assert!(!requests[0].contains("mutation"));
        assert!(
            api.qualification_review_threads("owner/invalid/repo", 7, None)
                .is_err()
        );
        assert!(
            api.qualification_review_threads("owner/repo", 0, None)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn graphql_errors_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
        let (origin, worker) = server(vec![reply(
            "200 OK",
            r#"{"errors":[{"message":"denied"}]}"#,
        )]);
        assert!(
            client(origin)
                .qualification_review_threads("owner/repo", 7, None)
                .is_err()
        );
        worker.join().map_err(|_| "server")?;
        Ok(())
    }
    #[test]
    fn rejects_invalid_pages_and_redirects() -> Result<(), Box<dyn std::error::Error>> {
        for body in [
            r#"{"data":null}"#,
            r#"{"data":null,"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[],"pageInfo":{"hasNextPage":true,"endCursor":null}}}}}}"#,
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"x","isResolved":true}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ] {
            let (origin, worker) = server(vec![reply("200 OK", body)]);
            assert!(
                client(origin)
                    .qualification_review_threads("owner/repo", 7, None)
                    .is_err()
            );
            worker.join().map_err(|_| "server")?;
        }
        let (origin, worker) = server(vec![reply("302 Found", "")]);
        assert!(
            client(origin)
                .qualification_review_threads("owner/repo", 7, None)
                .is_err()
        );
        worker.join().map_err(|_| "server")?;
        Ok(())
    }

    #[test]
    fn cursor_is_json_data_not_query_syntax() -> Result<(), Box<dyn std::error::Error>> {
        let (origin, worker) = server(vec![reply(
            "200 OK",
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        )]);
        let cursor = r#"\"mutation{evil}"#;
        client(origin).qualification_review_threads("owner/repo", 7, Some(cursor))?;
        let request = worker
            .join()
            .map_err(|_| "server")?
            .pop()
            .ok_or("request")?;
        let body = request.split_once("\r\n\r\n").ok_or("body")?.1;
        let parsed: serde_json::Value = serde_json::from_str(body)?;
        assert_eq!(parsed["variables"]["cursor"], cursor);
        assert!(
            !parsed["query"]
                .as_str()
                .ok_or("query")?
                .contains("mutation")
        );
        Ok(())
    }
    #[test]
    fn rejects_oversized_and_truncated_response() -> Result<(), Box<dyn std::error::Error>> {
        for response in [
            "HTTP/1.1 200 OK\r\nContent-Length: 8388609\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\n{",
        ] {
            let (origin, worker) = server(vec![response.to_owned()]);
            assert!(
                client(origin)
                    .qualification_review_threads("owner/repo", 7, None)
                    .is_err()
            );
            worker.join().map_err(|_| "server")?;
        }
        Ok(())
    }
    #[test]
    fn opinionated_reviews_use_fixed_read_only_query_and_validate_shape()
    -> Result<(), Box<dyn std::error::Error>> {
        let (origin, worker) = server(vec![reply(
            "200 OK",
            r#"{"data":{"repository":{"pullRequest":{"latestOpinionatedReviews":{"nodes":[{"id":"review-1","state":"APPROVED","submittedAt":"2026-11-01T00:00:00Z","author":{"__typename":"User","databaseId":42}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        )]);
        let page = client(origin).qualification_opinionated_reviews("owner/repo", 7, None)?;
        assert_eq!(
            page["data"]["repository"]["pullRequest"]["latestOpinionatedReviews"]["nodes"][0]["state"],
            "APPROVED"
        );
        let requests = worker.join().map_err(|_| "server")?;
        assert!(requests[0].contains("latestOpinionatedReviews"));
        assert!(!requests[0].contains("mutation"));
        let (origin, worker) = server(vec![reply(
            "200 OK",
            r#"{"data":{"repository":{"pullRequest":{"latestOpinionatedReviews":{"nodes":[{"id":"x","state":"APPROVED","submittedAt":"x","author":{"__typename":"User"}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        )]);
        assert!(
            client(origin)
                .qualification_opinionated_reviews("owner/repo", 7, None)
                .is_err()
        );
        worker.join().map_err(|_| "server")?;
        Ok(())
    }
    #[test]
    fn native_feedback_adapter_dispatches_only_fixed_queries()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::github_review_feedback::{FeedbackQuery, FeedbackRead};
        let (origin, worker) = server(vec![reply(
            "200 OK",
            r#"{"data":{"repository":{"pullRequest":{"latestOpinionatedReviews":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        )]);
        let api = crate::crate_release_github::NativeGitHubAdmissionRead {
            reader: client(origin),
        };
        let fields = [
            ("owner", "owner".into()),
            ("name", "repo".into()),
            ("number", "7".into()),
        ];
        api.feedback_graphql(FeedbackQuery::Opinions, &fields)?;
        let requests = worker.join().map_err(|_| "server")?;
        assert!(requests[0].contains("latestOpinionatedReviews"));
        let duplicate = [
            ("owner", "owner".into()),
            ("owner", "other".into()),
            ("name", "repo".into()),
            ("number", "7".into()),
        ];
        assert!(
            api.feedback_graphql(FeedbackQuery::Threads, &duplicate)
                .is_err()
        );
        Ok(())
    }
    #[test]
    fn native_transport_drives_complete_feedback_capture() -> Result<(), Box<dyn std::error::Error>>
    {
        use crate::github_review_feedback::{
            FeedbackContext, capture_feedback, verify_complete_feedback,
        };
        use std::collections::BTreeSet;
        use velnor_actions_contract::{
            canonical_json_bytes,
            homebrew_review_completion::{
                TAP_REVIEW_COMPLETION_SCHEMA, TapCompletedReviewer, TapReviewCompletionPayload,
                TapReviewVerdict,
            },
        };
        let head = "a".repeat(40);
        let pull = serde_json::json!({"number":7,"head":{"sha":head},"commits":1}).to_string();
        let commits = serde_json::json!([{"sha":head}]).to_string();
        let threads = r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#;
        let opinions = r#"{"data":{"repository":{"pullRequest":{"latestOpinionatedReviews":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#;
        let mut responses = Vec::new();
        for _ in 0..3 {
            for body in [
                pull.as_str(),
                commits.as_str(),
                "[]",
                "[]",
                "[]",
                "[]",
                threads,
                opinions,
                pull.as_str(),
            ] {
                responses.push(reply("200 OK", body));
            }
        }
        let (origin, worker) = server(responses);
        let api = crate::crate_release_github::NativeGitHubAdmissionRead {
            reader: client(origin),
        };
        let reviewers = BTreeSet::from([90]);
        let context = FeedbackContext {
            repository: "acme/repo",
            pull_request_number: 7,
            head_sha: &head,
            completion_app_id: 80,
            completion_check_name: "review-complete",
            expected_reviewer_app_ids: &reviewers,
        };
        let captured = capture_feedback(&api, &context, &[])?;
        let payload = TapReviewCompletionPayload {
            schema: TAP_REVIEW_COMPLETION_SCHEMA.into(),
            repository: "acme/repo".into(),
            pull_request_number: 7,
            head_sha: head.clone(),
            verdict: TapReviewVerdict::Complete,
            feedback_digest: captured.snapshot.canonical_digest()?,
            has_unaddressed_findings: false,
            reviewers: vec![TapCompletedReviewer {
                app_id: 90,
                head_sha: head.clone(),
                evidence_digest: format!("b3-{}", "c".repeat(64)),
                complete: true,
                has_unaddressed_findings: false,
            }],
        };
        let text = String::from_utf8(canonical_json_bytes(&payload)?)?;
        let checks = [
            serde_json::json!({"name":"review-complete","app":{"id":80},"head_sha":head,"status":"completed","conclusion":"success","output":{"text":text}}),
        ];
        let result = verify_complete_feedback(&api, &context, &checks)?;
        assert_eq!(result.records_digest, captured.snapshot.records_digest);
        assert_eq!(worker.join().map_err(|_| "server")?.len(), 27);
        Ok(())
    }
}
