// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::process::{Command, Output};

use crate::fetch::{Api, serve_body};
use crate::io::Repo;

type TestResult = Result<(), Box<dyn Error>>;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const OTHER_SHA: &str = "fedcba9876543210fedcba9876543210fedcba98";
const JOB_TOKEN: &str = "ghs_job_token";

fn head_ref(base: &str, number: u32, sha: &str) -> String {
    format!("refs/heads/gh-readonly-queue/{base}/pr-{number}-{sha}")
}

fn pull(repository: &str, sha: &str) -> String {
    format!(
        "{{\"number\":42,\"head\":{{\"sha\":\"{sha}\",\"repo\":{{\"full_name\":\"{repository}\"}}}}}}"
    )
}

struct Outcome {
    output: Output,
    written: String,
    api: Api,
}

impl Outcome {
    fn requests(&self) -> Vec<String> {
        self.api
            .requests
            .lock()
            .map(|requests| requests.clone())
            .unwrap_or_default()
    }
}

fn check(
    name: &str,
    head_ref: Option<&str>,
    status: u16,
    body: &str,
) -> Result<Outcome, Box<dyn Error>> {
    let repo = Repo::empty(name);
    let outputs = repo.root().join("github-output");
    let api = serve_body(status, body)?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .current_dir(repo.root())
        .arg("private-token-allowed")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GITHUB_OUTPUT", &outputs)
        .env("GITHUB_REPOSITORY", "acme/app")
        .env("GITHUB_TOKEN", JOB_TOKEN)
        .env("VELNOR_API_URL", &api.url);
    if let Some(head_ref) = head_ref {
        command.env("VELNOR_MERGE_GROUP_HEAD_REF", head_ref);
    }
    let output = command.output()?;
    let written = fs::read_to_string(&outputs).unwrap_or_default();
    Ok(Outcome {
        output,
        written,
        api,
    })
}

#[test]
fn a_same_repository_pull_request_in_the_queue_is_allowed_and_the_job_token_asks_for_it()
-> TestResult {
    let reference = head_ref("main", 42, SHA);
    let outcome = check("pc-same", Some(&reference), 200, &pull("acme/app", SHA))?;
    assert!(outcome.output.status.success(), "{:?}", outcome.output);
    assert_eq!(outcome.written, "allowed=true\n");
    assert_eq!(
        outcome.requests(),
        [format!(
            "get /repos/acme/app/pulls/42 http/1.1 | authorization: bearer {JOB_TOKEN}"
        )]
    );
    Ok(())
}

#[test]
fn a_base_branch_with_slashes_is_parsed_from_the_last_pr_segment() -> TestResult {
    let reference = head_ref("release/pr-7-x/1.2", 42, SHA);
    let outcome = check("pc-slash", Some(&reference), 200, &pull("acme/app", SHA))?;
    assert_eq!(outcome.written, "allowed=true\n");
    assert_eq!(outcome.requests().len(), 1);
    Ok(())
}

#[test]
fn a_fork_pull_request_in_the_queue_is_refused() -> TestResult {
    let reference = head_ref("main", 42, SHA);
    let outcome = check("pc-fork", Some(&reference), 200, &pull("mallory/app", SHA))?;
    assert!(outcome.output.status.success(), "{:?}", outcome.output);
    assert_eq!(outcome.written, "allowed=false\n");
    assert!(String::from_utf8_lossy(&outcome.output.stderr).contains("mallory/app"));
    Ok(())
}

#[test]
fn a_deleted_fork_repository_is_refused() -> TestResult {
    let reference = head_ref("main", 42, SHA);
    let body = format!("{{\"head\":{{\"sha\":\"{SHA}\",\"repo\":null}}}}");
    let outcome = check("pc-deleted", Some(&reference), 200, &body)?;
    assert_eq!(outcome.written, "allowed=false\n");
    Ok(())
}

#[test]
fn a_malformed_or_missing_head_ref_is_refused_without_a_lookup() -> TestResult {
    let bad = [
        None,
        Some(""),
        Some("refs/heads/main"),
        Some("refs/heads/gh-readonly-queue/main/pr-42"),
        Some("refs/heads/gh-readonly-queue/main/pr-x-0123456789abcdef0123456789abcdef01234567"),
        Some("refs/heads/gh-readonly-queue/main/pr-42-0123"),
        Some("refs/heads/gh-readonly-queue/main/pr-42-0123456789abcdef0123456789abcdef0123456789"),
        Some("refs/heads/gh-readonly-queue/main/pr-42-g123456789abcdef0123456789abcdef01234567"),
        Some("refs/heads/feature/pr-42-0123456789abcdef0123456789abcdef01234567"),
        Some("gh-readonly-queue//pr-42-0123456789abcdef0123456789abcdef01234567"),
    ];
    for (index, reference) in bad.into_iter().enumerate() {
        let outcome = check(
            &format!("pc-bad-{index}"),
            reference,
            200,
            &pull("acme/app", SHA),
        )?;
        assert!(outcome.output.status.success(), "{reference:?}");
        assert_eq!(outcome.written, "allowed=false\n", "{reference:?}");
        assert!(outcome.requests().is_empty(), "{reference:?}");
    }
    Ok(())
}

#[test]
fn a_head_ref_without_the_refs_heads_prefix_is_parsed() -> TestResult {
    let reference = format!("gh-readonly-queue/main/pr-42-{SHA}");
    let outcome = check("pc-bare", Some(&reference), 200, &pull("acme/app", SHA))?;
    assert_eq!(outcome.written, "allowed=true\n");
    Ok(())
}

#[test]
fn a_pull_request_head_that_differs_from_the_queue_sha_is_refused() -> TestResult {
    let reference = head_ref("main", 42, SHA);
    let outcome = check(
        "pc-sha",
        Some(&reference),
        200,
        &pull("acme/app", OTHER_SHA),
    )?;
    assert!(outcome.output.status.success(), "{:?}", outcome.output);
    assert_eq!(outcome.written, "allowed=false\n");
    Ok(())
}

#[test]
fn a_failed_lookup_or_an_unreadable_answer_is_refused() -> TestResult {
    let reference = head_ref("main", 42, SHA);
    for (index, (status, body)) in [
        (500, ""),
        (404, ""),
        (403, ""),
        (200, "not json"),
        (200, "{}"),
    ]
    .into_iter()
    .enumerate()
    {
        let outcome = check(&format!("pc-error-{index}"), Some(&reference), status, body)?;
        assert!(outcome.output.status.success(), "{status}");
        assert_eq!(outcome.written, "allowed=false\n", "{status} {body}");
    }
    Ok(())
}

#[test]
fn a_missing_token_or_api_is_refused() -> TestResult {
    let repo = Repo::empty("pc-env");
    let outputs = repo.root().join("github-output");
    let reference = head_ref("main", 42, SHA);
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("private-token-allowed")
        .env_clear()
        .env("GITHUB_OUTPUT", &outputs)
        .env("GITHUB_REPOSITORY", "acme/app")
        .env("VELNOR_MERGE_GROUP_HEAD_REF", reference)
        .output()?;
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&outputs)?, "allowed=false\n");
    Ok(())
}
