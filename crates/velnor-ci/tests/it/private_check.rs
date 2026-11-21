// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::process::{Command, Output};

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::fetch::Api;
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
        "{{\"number\":42,\"user\":{{\"login\":\"alice\"}},\"head\":{{\"sha\":\"{sha}\",\"repo\":{{\"full_name\":\"{repository}\"}}}}}}"
    )
}

const GROUP_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn serve_queue(status: u16, pull_body: &str, refs_body: String) -> Result<Api, Box<dyn Error>> {
    let pull_body = pull_body.to_owned();
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}", listener.local_addr()?);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buffer = [0_u8; 4096];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let text = String::from_utf8_lossy(&buffer[..read]).to_lowercase();
            let first = text.lines().next().unwrap_or_default().to_owned();
            let bearer = text
                .lines()
                .find(|line| line.starts_with("authorization:"))
                .unwrap_or_default()
                .to_owned();
            if let Ok(mut seen) = seen.lock() {
                seen.push(format!("{first} | {bearer}"));
            }
            let (code, body) = if first.contains("/git/matching-refs/") {
                (200, refs_body.as_str())
            } else {
                (status, pull_body.as_str())
            };
            drop(stream.write_all(
                format!(
                    "HTTP/1.1 {code} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            ));
        }
    });
    Ok(Api { url, requests })
}

fn queue_listing(head_ref: Option<&str>) -> String {
    let Some(head_ref) = head_ref else {
        return "[]".to_owned();
    };
    let name = head_ref.strip_prefix("refs/heads/").unwrap_or(head_ref);
    format!("[{{\"ref\":\"refs/heads/{name}\",\"object\":{{\"sha\":\"{GROUP_SHA}\"}}}}]")
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
            .map_or_default(|requests| requests.clone())
    }
}

fn check(
    name: &str,
    head_ref: Option<&str>,
    status: u16,
    body: &str,
) -> Result<Outcome, Box<dyn Error>> {
    check_at(name, head_ref, status, body, Some(GROUP_SHA))
}

fn check_at(
    name: &str,
    head_ref: Option<&str>,
    status: u16,
    body: &str,
    event_sha: Option<&str>,
) -> Result<Outcome, Box<dyn Error>> {
    let repo = Repo::empty(name);
    let outputs = repo.root().join("github-output");
    let api = serve_queue(status, body, queue_listing(head_ref))?;
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
    if let Some(event_sha) = event_sha {
        command.env("GITHUB_SHA", event_sha);
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
        [
            format!("get /repos/acme/app/pulls/42 http/1.1 | authorization: bearer {JOB_TOKEN}"),
            format!(
                "get /repos/acme/app/git/matching-refs/heads/gh-readonly-queue/main/?per_page=100 http/1.1 | authorization: bearer {JOB_TOKEN}"
            ),
        ]
    );
    Ok(())
}

#[test]
fn a_base_branch_with_slashes_is_parsed_from_the_last_pr_segment() -> TestResult {
    let reference = head_ref("release/pr-7-x/1.2", 42, SHA);
    let outcome = check("pc-slash", Some(&reference), 200, &pull("acme/app", SHA))?;
    assert_eq!(outcome.written, "allowed=true\n");
    assert_eq!(outcome.requests().len(), 2);
    Ok(())
}

#[test]
fn a_base_branch_with_reserved_characters_is_percent_encoded_in_the_listing_request() -> TestResult
{
    let reference = head_ref("feat#1/50%", 42, SHA);
    let outcome = check("pc-encode", Some(&reference), 200, &pull("acme/app", SHA))?;
    assert_eq!(outcome.written, "allowed=true\n");
    assert!(
        outcome.requests().iter().any(|request| request.starts_with(
            "get /repos/acme/app/git/matching-refs/heads/gh-readonly-queue/feat%231/50%25/?per_page=100 http/1.1"
        )),
        "{:?}",
        outcome.requests()
    );
    Ok(())
}

#[test]
fn a_queue_ref_rebuilt_since_the_event_is_refused() -> TestResult {
    let reference = head_ref("main", 42, SHA);
    let outcome = check_at(
        "pc-rebuilt",
        Some(&reference),
        200,
        &pull("acme/app", SHA),
        Some(OTHER_SHA),
    )?;
    assert!(outcome.output.status.success(), "{:?}", outcome.output);
    assert_eq!(outcome.written, "allowed=false\n");
    assert!(String::from_utf8_lossy(&outcome.output.stderr).contains("event"));
    Ok(())
}

#[test]
fn a_missing_event_sha_is_refused() -> TestResult {
    let reference = head_ref("main", 42, SHA);
    let outcome = check_at(
        "pc-nosha",
        Some(&reference),
        200,
        &pull("acme/app", SHA),
        None,
    )?;
    assert_eq!(outcome.written, "allowed=false\n");
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
    for (index, reference) in [None, Some("refs/heads/main")].into_iter().enumerate() {
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
    for (index, (status, body)) in [(500, ""), (200, "not json")].into_iter().enumerate() {
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
