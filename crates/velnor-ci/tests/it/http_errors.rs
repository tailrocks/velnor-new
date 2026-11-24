// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;

use crate::io::Repo;

struct Reply {
    status: u16,
    headers: Vec<(&'static str, &'static str)>,
    body: String,
}

fn serve(reply: Reply) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|e| unreachable!("{e}"));
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                request.push(byte[0]);
            }
            let mut lines = vec![format!("HTTP/1.1 {} Error", reply.status)];
            lines.extend(
                reply
                    .headers
                    .iter()
                    .map(|(name, value)| format!("{name}: {value}")),
            );
            lines.push(format!("content-length: {}", reply.body.len()));
            lines.push("connection: close".to_owned());
            let head = format!("{}\r\n\r\n", lines.join("\r\n"));
            stream.write_all(head.as_bytes()).ok();
            stream.write_all(reply.body.as_bytes()).ok();
        }
    });
    format!("http://{address}")
}

/// The stderr of a `cache trim` whose GitHub listing answers with `reply`.
fn failure_stderr(reply: Reply) -> String {
    let api = serve(reply);
    let repo = Repo::new("http-error-body");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["cache", "trim", "--prefix", "base-", "--keep", "0"])
        .env("GITHUB_TOKEN", "test-token")
        .env("GITHUB_REPOSITORY", "owner/name")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_API_URL", api)
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_403_reports_github_message_and_documentation_url() {
    let stderr = failure_stderr(Reply {
        status: 403,
        headers: vec![],
        body: r#"{"message":"Resource not accessible by integration","documentation_url":"https://docs.github.com/rest"}"#.to_owned(),
    });
    assert!(stderr.contains("http status: 403"), "stderr was: {stderr}");
    assert!(
        stderr.contains("Resource not accessible by integration"),
        "stderr was: {stderr}"
    );
    assert!(
        stderr.contains("docs.github.com/rest"),
        "stderr was: {stderr}"
    );
    assert!(!stderr.contains("test-token"), "stderr was: {stderr}");
}

#[test]
fn a_body_with_workflow_commands_cannot_inject_them() {
    let stderr = failure_stderr(Reply {
        status: 403,
        headers: vec![],
        body: "::set-output name=token::stolen\n::error::forged".to_owned(),
    });
    assert!(!stderr.contains("::set-output"), "stderr was: {stderr}");
    assert!(!stderr.contains("::error"), "stderr was: {stderr}");
    assert!(stderr.contains("set-output"), "stderr was: {stderr}");
}

#[test]
fn a_json_message_with_workflow_commands_cannot_inject_them() {
    let stderr = failure_stderr(Reply {
        status: 403,
        headers: vec![],
        body: r#"{"message":"::set-output name=token::stolen"}"#.to_owned(),
    });
    assert!(!stderr.contains("::set-output"), "stderr was: {stderr}");
    assert!(stderr.contains("set-output"), "stderr was: {stderr}");
}

#[test]
fn rate_limit_headers_are_reported() {
    let stderr = failure_stderr(Reply {
        status: 403,
        headers: vec![
            ("retry-after", "60"),
            ("x-ratelimit-remaining", "0"),
            ("x-ratelimit-reset", "1700000000"),
        ],
        body: r#"{"message":"API rate limit exceeded"}"#.to_owned(),
    });
    assert!(stderr.contains("retry-after: 60"), "stderr was: {stderr}");
    assert!(
        stderr.contains("x-ratelimit-remaining: 0"),
        "stderr was: {stderr}"
    );
    assert!(
        stderr.contains("x-ratelimit-reset: 1700000000"),
        "stderr was: {stderr}"
    );
}

#[test]
fn a_non_json_body_is_clipped() {
    let stderr = failure_stderr(Reply {
        status: 500,
        headers: vec![],
        body: format!("{}TAIL-MARKER", "x".repeat(2_000)),
    });
    assert!(stderr.contains("http status: 500"), "stderr was: {stderr}");
    assert!(!stderr.contains("TAIL-MARKER"), "stderr was: {stderr}");
}
