// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use serde_json::{Value, json};

use super::{ACCESS_TOKENS, CLIENT_ID, Key, app_token, minted_answer, serve};

type TestResult = Result<(), Box<dyn Error>>;

fn arguments(flags: &[&str]) -> Vec<String> {
    std::iter::once("--repository=shared-lib")
        .chain(flags.iter().copied())
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_token_request_carries_exactly_the_named_permissions_and_a_bad_one_makes_no_request()
-> TestResult {
    let key = Key::pkcs1("app-token-permissions")?;
    let api = serve((200, r#"{"id":4242}"#), (201, &minted_answer()))?;
    let output = key.path("github-output");
    let flags = [
        "--permission=contents:write",
        "--permission=pull_requests:read",
    ];
    let run = app_token(CLIENT_ID, &key.pem, &api, &output, &arguments(&flags));
    assert!(run.status.success(), "{run:?}");
    let requests = api.requests.lock().map_err(|e| e.to_string())?;
    assert_eq!(requests[1].line, format!("POST {ACCESS_TOKENS} HTTP/1.1"));
    let body: Value = serde_json::from_str(&requests[1].body)?;
    assert_eq!(
        body,
        json!({"repositories": ["shared-lib"], "permissions": {"contents": "write", "pull_requests": "read"}})
    );
    drop(requests);
    let refused: [&[&str]; 7] = [
        &["--permission=contents:admin"],
        &["--permission=contents"],
        &["--permission=:write"],
        &["--permission=Contents:write"],
        &["--permission=pull-requests:write"],
        &["--permission=contents:write:read"],
        &["--permission=contents:write", "--permission=contents:read"],
    ];
    for flags in refused {
        let api = serve((200, r#"{"id":4242}"#), (201, &minted_answer()))?;
        let output = key.path("refused-output");
        let run = app_token(CLIENT_ID, &key.pem, &api, &output, &arguments(flags));
        assert!(!run.status.success(), "{flags:?}: {run:?}");
        let requests = api.requests.lock().map_err(|e| e.to_string())?;
        assert!(requests.is_empty(), "{flags:?}: {}", requests.len());
        assert!(!output.exists(), "{flags:?}");
    }
    Ok(())
}
