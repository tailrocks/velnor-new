// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

use serde_json::json;
use velnor_local::io::authority::Authority;
use velnor_local::side::Side;

use super::{FakeRunners, TOKEN, post, run_facts};
use crate::support::temp_dir;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "git {args:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(repo: &Path, content: &str) -> String {
    let file = repo.join(".github/workflows/ci.yml");
    std::fs::create_dir_all(file.parent().unwrap_or(repo)).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(file, content).unwrap_or_else(|e| unreachable!("{e}"));
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "c"]);
    git(repo, &["rev-parse", "HEAD"])
}

fn sha256(content: &str) -> String {
    ring::digest::digest(&ring::digest::SHA256, content.as_bytes())
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

async fn answered(
    runners: &FakeRunners,
    repo: Option<&Path>,
    sha: &str,
    id: u64,
) -> serde_json::Value {
    let dir = temp_dir(&format!("authority-hashing-{id}"));
    let mut authority = Authority::open(runners, TOKEN.to_owned(), &dir.join("authority"))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    if let Some(repo) = repo {
        authority = authority.hashing(repo.to_path_buf());
    }
    let posted = json!({"id": "1-push", "side": "trusted", "event": "push",
        "ref": "refs/heads/main", "sha": sha, "workflow": null});
    assert_eq!(post(&authority, posted).await, 200);
    runners.create(Side::Trusted, id, "push", "refs/heads/main", sha);
    let (status, body) = run_facts(&authority, "trusted", id).await;
    assert_eq!(status, 200, "{body}");
    body
}

#[tokio::test]
async fn the_answer_carries_the_sha256_of_the_workflow_file_at_the_run_commit() {
    let repo = temp_dir("authority-hashing-repo");
    git(&repo, &["init", "-q", "-b", "main"]);
    let approved = commit(&repo, "name: ci\non: push\n");
    let changed = commit(&repo, "name: ci\non: push\n# edited by a fork\n");
    let runners = FakeRunners::default();
    let first = answered(&runners, Some(&repo), &approved, 1).await;
    assert_eq!(first["workflow_sha256"], sha256("name: ci\non: push\n"));
    let second = answered(&FakeRunners::default(), Some(&repo), &changed, 2).await;
    assert_eq!(
        second["workflow_sha256"],
        sha256("name: ci\non: push\n# edited by a fork\n")
    );
    assert_ne!(first["workflow_sha256"], second["workflow_sha256"]);
}

#[tokio::test]
async fn without_a_repository_or_with_a_commit_it_lacks_the_answer_has_no_hash() {
    let repo = temp_dir("authority-hashing-missing");
    git(&repo, &["init", "-q", "-b", "main"]);
    commit(&repo, "name: ci\n");
    let none = answered(&FakeRunners::default(), None, &"a".repeat(40), 3).await;
    assert!(none["workflow_sha256"].is_null(), "{none}");
    let absent = answered(&FakeRunners::default(), Some(&repo), &"b".repeat(40), 4).await;
    assert!(absent["workflow_sha256"].is_null(), "{absent}");
    let option = answered(&FakeRunners::default(), Some(&repo), "--output=x", 5).await;
    assert!(option["workflow_sha256"].is_null(), "{option}");
}
