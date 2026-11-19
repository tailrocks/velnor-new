// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::symlink;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::*;

const MEMBERS_BYTES: u64 = 11_760;
const PLAN_LIMIT: usize = 8 << 20;

fn fetch_within(served: &Served, dir: &Path, extra: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["fetch-reports", "--repo", REPO, "--run-id"])
        .arg(RUN_ID.to_string())
        .arg("--run-dir")
        .arg(dir)
        .args(extra)
        .env("GITHUB_API_URL", &served.api)
        .env("GITHUB_TOKEN", "fixture-token")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().ok().flatten().is_none() {
        if Instant::now() > deadline {
            child.kill().ok();
            child.wait().ok();
            unreachable!("fetch-reports did not finish within 30s");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    child
        .wait_with_output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn extracted_bytes(dir: &Path) -> u64 {
    tree(&dir.join("reports"))
        .values()
        .map(|content| content.len() as u64)
        .sum()
}

fn budget_arg(bytes: u64) -> [String; 2] {
    ["--max-extract-bytes".to_owned(), bytes.to_string()]
}

#[test]
fn one_extraction_budget_is_shared_by_every_artifact() {
    let total = 31 * MEMBERS_BYTES;
    let served = serve(routes(&recorded(), ARCHIVE));
    let dir = run_dir("budget-enough", Some(PLAN));
    let [flag, value] = budget_arg(total);
    let output = fetch_within(&served, &dir, &[&flag, &value]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(extracted_bytes(&dir), total);

    let served = serve(routes(&recorded(), ARCHIVE));
    let dir = run_dir("budget-short", Some(PLAN));
    let [flag, value] = budget_arg(total - 1);
    let output = fetch_within(&served, &dir, &[&flag, &value]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let message = stderr(&output);
    assert!(
        message.contains(" of 31 ") && message.contains("extraction budget"),
        "{message}"
    );
    assert!(extracted_bytes(&dir) < total);
    assert!(fetched(&dir).len() < 31);
}

#[test]
fn a_small_budget_stops_the_fetch_long_before_every_artifact_is_extracted() {
    let served = serve(routes(&recorded(), ARCHIVE));
    let dir = run_dir("budget-small", Some(PLAN));
    let budget = 3 * MEMBERS_BYTES;
    let [flag, value] = budget_arg(budget);
    let output = fetch_within(&served, &dir, &[&flag, &value]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains("extraction budget"));
    assert!(extracted_bytes(&dir) <= budget);
    assert!(fetched(&dir).len() <= 3);
}

#[test]
fn two_live_artifacts_with_one_expected_name_are_a_selection_error() {
    let mut listing = recorded();
    let name = "velnor-crate-r44491127982-a1-rust-velnor-gen";
    let first = id_of(&listing, name);
    let twin = json!({"id": first + 1, "name": name, "expired": false});
    listing["artifacts"]
        .as_array_mut()
        .unwrap_or_else(|| unreachable!("listing has artifacts"))
        .push(twin);
    let served = serve(routes(&listing, ARCHIVE));
    let dir = run_dir("duplicate-live", Some(PLAN));
    let output = fetch_within(&served, &dir, &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let message = stderr(&output);
    assert!(
        message.contains(name) && message.contains("2 live artifacts"),
        "{message}"
    );
    assert!(!fetched(&dir).contains(name));
    assert_eq!(fetched(&dir).len(), 30);
    assert_eq!(served.count("/blob/"), 30);
}

#[test]
fn a_duplicate_key_in_plan_json_is_refused_before_any_request() {
    let name = "velnor-crate-r44491127982-a1-rust-velnor-gen";
    let plans = [
        format!(r#"{{"matrix":{{"include":[{{"artifact_id":"{name}","artifact_id":"other"}}]}}}}"#),
        format!(
            r#"{{"matrix":{{"include":[]}},"matrix":{{"include":[{{"artifact_id":"{name}"}}]}}}}"#
        ),
    ];
    let served = serve(routes(&recorded(), ARCHIVE));
    for plan in plans {
        let dir = run_dir("duplicate-key", Some(&plan));
        let output = fetch_within(&served, &dir, &[]);
        assert_eq!(output.status.code(), Some(1), "{plan}: {output:?}");
        let message = stderr(&output);
        assert!(
            message.contains("plan.json") && message.contains("repeats"),
            "{message}"
        );
    }
    assert_eq!(served.count(""), 0);
}

#[test]
fn a_plan_json_that_is_a_fifo_fails_instead_of_blocking() {
    let served = serve(routes(&recorded(), ARCHIVE));
    let dir = run_dir("fifo", None);
    let made = Command::new("mkfifo").arg(dir.join("plan.json")).status();
    assert!(made.is_ok_and(|status| status.success()));
    let output = fetch_within(&served, &dir, &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains("not a regular file"));
    assert_eq!(served.count(""), 0);
}

#[test]
fn a_plan_json_that_is_a_symbolic_link_is_refused() {
    let served = serve(routes(&recorded(), ARCHIVE));
    let real = run_dir("symlink-target", Some(PLAN)).join("plan.json");
    let dir = run_dir("symlink", None);
    symlink(&real, dir.join("plan.json")).unwrap_or_else(|e| unreachable!("{e}"));
    let output = fetch_within(&served, &dir, &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains("plan.json"));
    assert_eq!(served.count(""), 0);
}

#[test]
fn a_plan_json_over_the_size_limit_is_refused_even_when_it_is_valid() {
    let served = serve(routes(&recorded(), ARCHIVE));
    let padded = format!("{PLAN}{}", " ".repeat(PLAN_LIMIT));
    let dir = run_dir("oversized", Some(&padded));
    let output = fetch_within(&served, &dir, &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains("larger than"));
    assert_eq!(served.count(""), 0);
}

#[test]
fn a_transient_listing_failure_is_retried_and_a_persistent_one_fails_after_three_attempts() {
    let listing = recorded();
    let first = listing_path(1);
    let mut table = routes(&listing, ARCHIVE);
    let page = table[&first][0].clone();
    table.insert(first.clone(), vec![Reply::status(503), page]);
    let served = serve(table);
    let dir = run_dir("listing-flaky", Some(PLAN));
    let output = fetch_within(&served, &dir, &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(served.count(&first), 2);

    let mut table = routes(&listing, ARCHIVE);
    table.insert(first.clone(), vec![Reply::status(503)]);
    let served = serve(table);
    let dir = run_dir("listing-dead", Some(PLAN));
    let output = fetch_within(&served, &dir, &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(served.count(&first), 3);
    assert!(stderr(&output).contains("3 attempts"));
    assert_eq!(served.count("/blob/"), 0);
}

#[test]
fn a_transient_failure_on_a_later_listing_page_is_retried() {
    let listing = recorded();
    let all = listed(&listing);
    let (head, rest) = all.split_at(20);
    let page = |items: &[Value]| json!({"total_count": all.len(), "artifacts": items}).to_string();
    let mut table = routes(&listing, ARCHIVE);
    table.insert(listing_path(1), vec![Reply::ok(page(head))]);
    table.insert(
        listing_path(2),
        vec![Reply::status(502), Reply::ok(page(rest))],
    );
    let served = serve(table);
    let dir = run_dir("listing-page-two", Some(PLAN));
    let output = fetch_within(&served, &dir, &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(served.count(&listing_path(1)), 1);
    assert_eq!(served.count(&listing_path(2)), 2);
}
