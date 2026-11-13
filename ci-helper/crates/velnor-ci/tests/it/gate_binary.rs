// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::thread;

use crate::gate::{OFF_MAIN_GREEN, OFF_MAIN_RED, ON_MAIN_GREEN, ON_MAIN_RED};
use crate::support::GATE;

fn serve(responses: BTreeMap<String, String>) -> String {
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
            let text = String::from_utf8_lossy(&request).into_owned();
            let path = text.split_whitespace().nth(1).unwrap_or_default();
            let (status, body) = responses
                .get(path)
                .map_or(("404 Not Found", String::new()), |body| {
                    ("200 OK", body.clone())
                });
            let reply = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).ok();
        }
    });
    format!("http://{address}")
}

fn release_gate(api: &str, token: Option<&str>, sha: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args([
            "release-gate",
            "--repo",
            "tailrocks/velnor-new",
            "--sha",
            sha,
        ])
        .env("GITHUB_API_URL", api);
    match token {
        Some(token) => command.env("GITHUB_TOKEN", token),
        None => command.env_remove("GITHUB_TOKEN"),
    };
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn release_gate_exits_0_for_a_released_commit_and_1_for_each_refusal() {
    let recorded: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(GATE).unwrap_or_else(|e| unreachable!("{e}"));
    let api = serve(
        recorded
            .into_iter()
            .map(|(path, body)| (path, body.to_string()))
            .collect(),
    );
    let cases = [
        (ON_MAIN_GREEN, Some(0), "check Required passed", ""),
        (
            OFF_MAIN_GREEN,
            Some(1),
            "",
            "not on the default branch main",
        ),
        (ON_MAIN_RED, Some(1), "", "it concluded failure"),
        (OFF_MAIN_RED, Some(1), "", "not on the default branch main"),
    ];
    for (sha, code, said, complained) in cases {
        let output = release_gate(&api, Some("recorded"), sha);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), code, "{sha}: {stdout}{stderr}");
        assert!(stdout.contains(said), "{sha}: {stdout}");
        assert!(stderr.contains(complained), "{sha}: {stderr}");
    }
}

#[test]
fn release_gate_without_a_token_exits_1_before_any_request() {
    let output = release_gate("http://127.0.0.1:1", None, ON_MAIN_GREEN);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("set GITHUB_TOKEN"));
}
