// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_ci::time::date_text;

use crate::io::Repo;
use crate::io_plan::{plan_command, plan_repo, selected};

const DAY: i64 = 86_400;

fn today(days_ago: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0));
    format!("{}\n", date_text(now - days_ago * DAY))
}

fn restored(repo: &Repo, days: Option<&str>) -> (Output, String) {
    let outputs = repo.root().join("github-output");
    fs::remove_file(&outputs).unwrap_or_default();
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .arg("restored")
        .current_dir(repo.root())
        .env("GITHUB_OUTPUT", &outputs);
    if let Some(days) = days {
        command.env("VELNOR_BASE_MAX_AGE_DAYS", days);
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    (output, fs::read_to_string(outputs).unwrap_or_default())
}

fn built(name: &str, born: Option<&str>) -> Repo {
    let repo = Repo::new(name);
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.write_unstaged("target/debug/deps/libapp.rlib", "rlib");
    if let Some(born) = born {
        repo.write_unstaged("target/.velnor/born", born);
    }
    repo
}

#[test]
fn restored_reports_a_fresh_base_as_not_expired_and_keeps_it() {
    let repo = built("report-fresh", Some(&today(1)));
    let (output, written) = restored(&repo, Some("7"));
    assert!(output.status.success());
    assert_eq!(written, "expired=false\n");
    assert!(repo.root().join("target/debug/deps/libapp.rlib").is_file());
}

#[test]
fn restored_reports_an_old_base_as_expired_and_removes_it() {
    let repo = built("report-old", Some(&today(8)));
    let (output, written) = restored(&repo, Some("7"));
    assert!(output.status.success());
    assert_eq!(written, "expired=true\n");
    assert!(!repo.root().join("target").exists());
}

#[test]
fn restored_reports_an_unreadable_age_as_expired() {
    for (name, born) in [("report-missing", None), ("report-garbage", Some("soon\n"))] {
        let repo = built(name, born);
        let (output, written) = restored(&repo, Some("7"));
        assert!(output.status.success());
        assert_eq!(written, "expired=true\n", "{name}");
        assert!(!repo.root().join("target").exists(), "{name}");
    }
}

#[test]
fn restored_reports_no_expiry_without_a_limit_or_a_build_directory() {
    let repo = built("report-unlimited", Some("2020-01-01\n"));
    for days in [None, Some("0")] {
        assert_eq!(restored(&repo, days).1, "expired=false\n");
    }
    let bare = Repo::new("report-bare");
    assert_eq!(restored(&bare, Some("7")).1, "expired=false\n");
}

fn affected_push(name: &str) -> Output {
    let repo = plan_repo(name);
    repo.write(".velnor/rust-ci.toml", "full_run = \"off\"\n");
    repo.commit();
    repo.git(&["branch", "-M", "main"]);
    repo.write("docs/guide.md", "guide\n");
    repo.commit();
    let head = String::from_utf8_lossy(
        &Command::new("git")
            .arg("-C")
            .arg(repo.root())
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"))
            .stdout,
    )
    .trim()
    .to_owned();
    let before = format!("{head}^1");
    let payload = format!(
        "{{\"before\":\"{}\",\"after\":\"{head}\",\"repository\":{{\"default_branch\":\"main\"}}}}",
        String::from_utf8_lossy(
            &Command::new("git")
                .arg("-C")
                .arg(repo.root())
                .args(["rev-parse", &before])
                .output()
                .unwrap_or_else(|e| unreachable!("{e}"))
                .stdout
        )
        .trim()
    );
    let file = repo.root().join("event.json");
    fs::write(&file, payload).unwrap_or_else(|e| unreachable!("{e}"));
    let mut plan = plan_command(&repo);
    plan.args(["plan", "--explain"]);
    if name.ends_with("full") {
        plan.arg("--full");
    }
    plan.env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_EVENT_PATH", file)
        .env("GITHUB_SHA", head)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn plan_full_selects_everything_on_an_affected_only_push_and_plain_plan_does_not() {
    let partial = affected_push("expiry-plan-partial");
    assert!(partial.status.success(), "{partial:?}");
    assert!(
        selected(&partial).contains("none"),
        "{}",
        selected(&partial)
    );
    let full = affected_push("expiry-plan-full");
    assert!(full.status.success(), "{full:?}");
    let chosen = selected(&full);
    assert!(!chosen.contains("none"), "{chosen}");
    assert!(chosen.contains("all:"), "{chosen}");
}
