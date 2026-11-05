// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::tests::{client, reply, server};

#[test]
fn fixed_query_reads_threads_without_accepting_mutations() -> Result<(), Box<dyn std::error::Error>>
{
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
fn provider_query_accepts_only_fixed_operations_and_bounded_fields()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::tap_merge::review_completion::provider::ProviderQuery;
    let (origin, worker) = server(vec![reply("200 OK", r#"{"data":{"node":{"id":"node"}}}"#)]);
    let api = client(origin);
    let value =
        api.qualification_provider_graphql(ProviderQuery::IssueEditor, &[("id", "node".into())])?;
    assert_eq!(value["data"]["node"]["id"], "node");
    let requests = worker.join().map_err(|_| "server")?;
    let body = requests[0].split_once("\r\n\r\n").ok_or("body")?.1;
    let request: serde_json::Value = serde_json::from_str(body)?;
    assert_eq!(request["query"], ProviderQuery::IssueEditor.query());
    assert_eq!(request["variables"]["id"], "node");
    assert!(
        api.qualification_provider_graphql(
            ProviderQuery::IssueEditor,
            &[("id", "node".into()), ("cursor", "extra".into())]
        )
        .is_err()
    );
    assert!(
        api.qualification_provider_graphql(
            ProviderQuery::ThreadComments,
            &[("id", "x".repeat(513))]
        )
        .is_err()
    );
    assert!(
        api.qualification_provider_graphql(
            ProviderQuery::ThreadComments,
            &[("id", "node".into()), ("id", "other".into())]
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn deleted_comment_query_binds_repository_and_pr() -> Result<(), Box<dyn std::error::Error>> {
    use crate::tap_merge::review_completion::provider::ProviderQuery;
    let (origin, worker) = server(vec![reply(
        "200 OK",
        r#"{"data":{"repository":{"fullDatabaseId":"12","nameWithOwner":"acme/example","pullRequest":{"number":7,"timelineItems":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
    )]);
    let api = client(origin);
    let fields = [
        ("owner", "acme".into()),
        ("name", "example".into()),
        ("number", "7".into()),
    ];
    assert!(
        api.qualification_provider_graphql(ProviderQuery::DeletedComments, &fields)
            .is_ok()
    );
    let requests = worker.join().map_err(|_| "server")?;
    let body = requests[0].split_once("\r\n\r\n").ok_or("body")?.1;
    let request: serde_json::Value = serde_json::from_str(body)?;
    assert_eq!(request["variables"]["number"], 7);
    assert_eq!(request["query"], ProviderQuery::DeletedComments.query());
    assert!(
        api.qualification_provider_graphql(
            ProviderQuery::DeletedComments,
            &[
                ("owner", "acme".into()),
                ("name", "example".into()),
                ("number", "07".into())
            ]
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn provider_app_reads_are_exactly_scoped() -> Result<(), Box<dyn std::error::Error>> {
    let (origin, worker) = server(vec![reply("200 OK", r#"{"id":1144995}"#)]);
    let api = client(origin);
    assert_eq!(api.get("apps/chatgpt-codex-connector")?["id"], 1_144_995);
    assert!(
        api.get("apps/chatgpt-codex-connector/installation")
            .is_err()
    );
    assert!(api.get("apps/other").is_err());
    assert_eq!(worker.join().map_err(|_| "server")?.len(), 1);
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
fn native_feedback_adapter_dispatches_only_fixed_queries() -> Result<(), Box<dyn std::error::Error>>
{
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
fn native_transport_drives_complete_feedback_capture() -> Result<(), Box<dyn std::error::Error>> {
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
