// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use crate::gate::{AHEAD, GATE, GREEN, RED, RUNNING};
use crate::io::{Repo, serve};

fn release_gate(api: &str, token: Option<&str>, sha: &str, outputs: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args([
            "release-gate",
            "--repo",
            "tailrocks/velnor-new",
            "--sha",
            sha,
        ])
        .env("GITHUB_API_URL", api)
        .env("GITHUB_OUTPUT", outputs);
    match token {
        Some(token) => command.env("GITHUB_TOKEN", token),
        None => command.env_remove("GITHUB_TOKEN"),
    };
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn release_gate_writes_ready_and_exits_0_until_a_refusal_exits_1() {
    let recorded: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(GATE).unwrap_or_else(|e| unreachable!("{e}"));
    let api = serve(
        recorded
            .into_iter()
            .map(|(path, body)| (path, body.to_string()))
            .collect(),
    );
    let cases = [
        (GREEN, Some(0), "ready=true\n", ""),
        (RUNNING, Some(0), "ready=false\n", ""),
        (RED, Some(1), "", "it concluded failure"),
        (AHEAD, Some(1), "", "not on the default branch main"),
    ];
    let repo = Repo::new("release-gate");
    for (sha, code, ready, complained) in cases {
        let outputs = repo.root().join(format!("github-output-{sha}"));
        let output = release_gate(&api, Some("recorded"), sha, &outputs);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), code, "{sha}: {stdout}{stderr}");
        assert!(stdout.ends_with(ready), "{sha}: {stdout}");
        assert!(stderr.contains(complained), "{sha}: {stderr}");
        let written = fs::read_to_string(&outputs).unwrap_or_default();
        assert_eq!(written, ready, "{sha}");
    }
}

#[test]
fn release_gate_without_a_token_exits_1_before_any_request() {
    let output = release_gate("http://127.0.0.1:1", None, GREEN, Path::new(""));
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("set GITHUB_TOKEN"));
}

#[test]
fn check_tag_exits_0_for_the_tag_of_its_own_version_and_1_for_any_other() {
    let version = env!("CARGO_PKG_VERSION");
    for (tag, code) in [
        (format!("rust-ci-v{version}"), Some(0)),
        (format!("tools-v{version}"), Some(0)),
        ("rust-ci-v0.0.0".to_owned(), Some(1)),
        ("tools-v0.0.0".to_owned(), Some(1)),
        (format!("v{version}"), Some(1)),
        (format!("runner-v{version}"), Some(1)),
        (format!("actions-v{version}"), Some(1)),
        (format!("x{version}"), Some(1)),
        (format!("rust-ci-{version}"), Some(1)),
        (version.to_owned(), Some(1)),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["check-tag", &tag])
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(output.status.code(), code, "{tag}");
    }
}
