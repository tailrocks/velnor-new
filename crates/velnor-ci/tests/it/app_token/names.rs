// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

use super::{Key, mint, mint_as, minted_answer, serve};

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn a_dotted_client_id_is_accepted_and_becomes_the_jwt_issuer() -> TestResult {
    let key = Key::pkcs1("app-token-dotted-client")?;
    let api = serve((200, r#"{"id":4242}"#), (201, &minted_answer()))?;
    let output = key.path("github-output");
    let run = mint_as(
        "Iv1.ab1112223334445c",
        &key.pem,
        &api,
        &output,
        &["shared-lib"],
    );
    assert!(run.status.success(), "{run:?}");
    let requests = api.requests.lock().map_err(|e| e.to_string())?;
    let jwt = requests[0].authorization.trim_start_matches("Bearer ");
    let claims: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(jwt.split('.').nth(1).ok_or("claims")?)?)?;
    assert_eq!(claims["iss"], "Iv1.ab1112223334445c");
    Ok(())
}

#[test]
fn a_client_id_that_could_inject_into_a_header_or_url_makes_no_request() -> TestResult {
    let key = Key::pkcs1("app-token-bad-client")?;
    for client_id in [
        "Iv1.a b", "Iv1/a", "Iv1.a\nb", "Iv1.a?b", "Iv1.a\"b", "a%2Fb", ".", "Iv1.a;b",
    ] {
        let api = serve((200, r#"{"id":4242}"#), (201, &minted_answer()))?;
        let output = key.path("github-output");
        let run = mint_as(client_id, &key.pem, &api, &output, &["shared-lib"]);
        assert!(!run.status.success(), "{client_id}: {run:?}");
        let requests = api.requests.lock().map_err(|e| e.to_string())?;
        assert!(requests.is_empty(), "{client_id}");
        assert!(!output.exists());
    }
    Ok(())
}

#[test]
fn a_leading_dot_repository_is_looked_up_by_its_exact_name() -> TestResult {
    let key = Key::pkcs1("app-token-leading-dot")?;
    let api = serve((200, r#"{"id":4242}"#), (201, &minted_answer()))?;
    let output = key.path("github-output");
    let run = mint(&key.pem, &api, &output, &[".github-private", "shared-lib"]);
    assert!(run.status.success(), "{run:?}");
    let requests = api.requests.lock().map_err(|e| e.to_string())?;
    assert_eq!(
        requests[0].line,
        "GET /repos/acme/.github-private/installation HTTP/1.1"
    );
    let body: Value = serde_json::from_str(&requests[1].body)?;
    assert_eq!(
        body["repositories"],
        json!([".github-private", "shared-lib"])
    );
    Ok(())
}

#[test]
fn dot_segments_and_path_like_repository_names_make_no_request() -> TestResult {
    let key = Key::pkcs1("app-token-dot-segments")?;
    for name in [
        ".", "..", "...", "a/b", "a/../b", "a\\b", "a b", "a?b", "a#b", "a%2e", "",
    ] {
        let api = serve((200, r#"{"id":4242}"#), (201, &minted_answer()))?;
        let output = key.path("github-output");
        let run = mint(&key.pem, &api, &output, &[name]);
        assert!(!run.status.success(), "{name}: {run:?}");
        let requests = api.requests.lock().map_err(|e| e.to_string())?;
        assert!(requests.is_empty(), "{name}");
    }
    Ok(())
}
