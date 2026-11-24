// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::process::{Command, Output};

use super::{Api, MINTED, minted_answer, serve};

type TestResult = Result<(), Box<dyn Error>>;

fn revoke(api: &Api, token: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .env_clear()
        .arg("revoke-token")
        .env("VELNOR_APP_TOKEN", token)
        .env("VELNOR_API_URL", &api.url)
        .output()
        .unwrap_or_else(|e| unreachable!("velnor-ci: {e}"))
}

#[test]
fn the_token_is_revoked_on_the_configured_api_and_a_missing_token_makes_no_request() -> TestResult {
    let api = serve((200, r#"{"id":4242}"#), (201, &minted_answer()))?;
    let run = revoke(&api, MINTED);
    assert!(run.status.success(), "{run:?}");
    let requests = api.requests.lock().map_err(|e| e.to_string())?;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].line, "DELETE /installation/token HTTP/1.1");
    assert_eq!(requests[0].authorization, format!("Bearer {MINTED}"));
    for log in [&run.stdout, &run.stderr] {
        assert!(!String::from_utf8_lossy(log).contains(MINTED), "{run:?}");
    }
    drop(requests);
    let api = serve((200, r#"{"id":4242}"#), (201, &minted_answer()))?;
    let run = revoke(&api, "");
    assert!(!run.status.success(), "{run:?}");
    assert!(api.requests.lock().map_err(|e| e.to_string())?.is_empty());
    Ok(())
}
