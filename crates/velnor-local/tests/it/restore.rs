// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use http_body_util::BodyExt;
use velnor_local::io::{Body, RestoreOnly};
use velnor_local::side::fork_may_restore;

use crate::support::{token, token_key};

async fn handled(_: hyper::Request<Body>) -> hyper::Response<Body> {
    let body: Body = http_body_util::Empty::new()
        .map_err(|never| match never {})
        .boxed_unsync();
    let mut response = hyper::Response::new(body);
    *response.status_mut() = hyper::StatusCode::NO_CONTENT;
    response
}

fn request(method: &str, path: &str, git_ref: Option<&str>) -> hyper::Request<Body> {
    let mut builder = hyper::Request::builder().method(method).uri(path);
    if let Some(git_ref) = git_ref {
        builder = builder.header("authorization", format!("Bearer {}", token(git_ref)));
    }
    builder
        .body(
            http_body_util::Empty::new()
                .map_err(|never| match never {})
                .boxed_unsync(),
        )
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[tokio::test]
async fn the_untrusted_cache_server_restores_only_for_the_fork_pull_requests_registered_with_it() {
    let server = RestoreOnly::new(token_key());
    let twirp = "/twirp/github.actions.results.api.v1.CacheService/";
    let restore = format!("{twirp}GetCacheEntryDownloadURL");
    let fork = Some("refs/pull/2/merge");
    for (method, path, git_ref, status) in [
        ("GET", "/velnor-cache/archives/abc".to_owned(), None, 204),
        ("GET", "/_apis/v1/Message".to_owned(), None, 204),
        ("POST", restore.clone(), fork, 403),
        ("PUT", "/_velnor/pull-requests/2".to_owned(), None, 204),
        ("POST", restore.clone(), fork, 204),
        ("POST", restore.clone(), Some("refs/pull/1/merge"), 403),
        ("POST", restore.clone(), Some("refs/heads/main"), 403),
        ("POST", restore.clone(), Some("expired"), 403),
        ("POST", restore.clone(), None, 403),
        ("POST", format!("{twirp}CreateCacheEntry"), fork, 403),
        (
            "POST",
            format!("{twirp}FinalizeCacheEntryUpload"),
            fork,
            403,
        ),
        ("PUT", "/velnor-cache/archives/abc".to_owned(), fork, 403),
    ] {
        let response = server
            .handle(request(method, &path, git_ref), handled)
            .await;
        assert_eq!(
            response.status().as_u16(),
            status,
            "{method} {path} {git_ref:?}"
        );
    }
}

#[test]
fn only_the_merge_ref_of_a_registered_fork_pull_request_may_restore() {
    let forks = BTreeSet::from([2]);
    assert!(fork_may_restore("refs/pull/2/merge", &forks));
    for refused in [
        "refs/pull/1/merge",
        "refs/pull/2/head",
        "refs/heads/main",
        "refs/pull/x/merge",
    ] {
        assert!(!fork_may_restore(refused, &forks), "{refused}");
    }
}
