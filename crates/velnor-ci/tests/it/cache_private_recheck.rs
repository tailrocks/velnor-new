// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Command;

use crate::fetch::{TestResult, serve_body};
use crate::io::Repo as Checkout;

const TOKEN: &str = "ghs_job_token";
const NOT_FOUND: &str = r#"{"message":"Not Found"}"#;
const PUBLIC: &str = r#"{"full_name":"acme/app","private":false,"visibility":"public"}"#;
const OTHER: &str = r#"{"full_name":"acme/other","private":false,"visibility":"public"}"#;

fn recheck(
    name: &str,
    status: u16,
    body: &str,
) -> Result<(String, Vec<String>), Box<dyn std::error::Error>> {
    let repo = Checkout::empty(name);
    let outputs = repo.root().join("github-output");
    let api = serve_body(status, body)?;
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .args(["cache", "private-recheck"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GITHUB_OUTPUT", &outputs)
        .env("GITHUB_REPOSITORY", "acme/app")
        .env("GITHUB_TOKEN", TOKEN)
        .env("VELNOR_API_URL", &api.url)
        .output()?;
    assert!(output.status.success(), "{name}: {output:?}");
    let logged = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!logged.contains(TOKEN), "{name}: {logged}");
    let requests = api
        .requests
        .lock()
        .map_or_else(|_| Vec::new(), |requests| requests.clone());
    Ok((fs::read_to_string(&outputs).unwrap_or_default(), requests))
}

#[test]
fn a_repository_that_answers_not_found_without_a_token_is_still_private() -> TestResult {
    let (written, requests) = recheck("recheck-private", 404, NOT_FOUND)?;
    assert_eq!(written, "private=true\n");
    assert_eq!(requests, ["get /repos/acme/app http/1.1 | "]);
    Ok(())
}

#[test]
fn a_repository_that_can_be_read_without_a_token_is_no_longer_private() -> TestResult {
    let (written, requests) = recheck("recheck-public", 200, PUBLIC)?;
    assert_eq!(written, "private=false\n");
    assert_eq!(requests, ["get /repos/acme/app http/1.1 | "]);
    Ok(())
}

#[test]
fn a_lookup_that_proves_nothing_refuses_the_save_and_is_reported_unavailable() -> TestResult {
    for (label, status, body) in [
        ("rate-limited", 403, NOT_FOUND),
        ("server-error", 500, NOT_FOUND),
        ("malformed", 200, "not json"),
        ("other-repository", 200, OTHER),
    ] {
        let (written, _) = recheck(&format!("recheck-{label}"), status, body)?;
        assert_eq!(written, "private=false\nunavailable=true\n", "{label}");
    }
    Ok(())
}
