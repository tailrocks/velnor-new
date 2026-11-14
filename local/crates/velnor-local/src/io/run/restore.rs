// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::future::Future;
use std::time::{SystemTime, UNIX_EPOCH};

use hyper::header::AUTHORIZATION;
use hyper::{Method, Request, Response};
use serde_json::json;
use tokio::sync::Mutex;

use crate::io::{Body, json_response};
use crate::protocol::{Route, route, writes};
use crate::side::fork_may_restore;
use crate::token;

#[derive(Debug)]
pub struct RestoreOnly {
    token_key: Vec<u8>,
    forks: Mutex<BTreeSet<u64>>,
}

impl RestoreOnly {
    #[must_use]
    pub fn new(token_key: Vec<u8>) -> Self {
        Self {
            token_key,
            forks: Mutex::new(BTreeSet::new()),
        }
    }

    pub async fn handle<H, F>(&self, request: Request<Body>, handler: H) -> Response<Body>
    where
        H: FnOnce(Request<Body>) -> F,
        F: Future<Output = Response<Body>>,
    {
        let path = request.uri().path().to_owned();
        if let (&Method::PUT, Route::PullRequest(number)) = (request.method(), route(&path)) {
            if let Ok(number) = number.parse() {
                self.forks.lock().await.insert(number);
            }
            return handler(request).await;
        }
        if writes(request.method().as_str(), &path) {
            return refuse(
                "this cache server serves restores only, as GitHub does for pull requests from forks",
            );
        }
        if let Route::Twirp(_) = route(&path) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_secs());
            let claims = request
                .headers()
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
                .and_then(|bearer| token::verify(bearer, &self.token_key, now).ok());
            let forks = self.forks.lock().await;
            if !claims.is_some_and(|claims| fork_may_restore(&claims.git_ref, &forks)) {
                return refuse(
                    "this cache server serves only the pull requests from forks registered with it",
                );
            }
        }
        handler(request).await
    }
}

fn refuse(message: &str) -> Response<Body> {
    json_response(403, &json!({ "message": message }))
}
