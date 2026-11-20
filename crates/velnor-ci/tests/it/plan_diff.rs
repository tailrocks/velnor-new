// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! `velnor-ci plan --mode ci --request` for `pull_request`, fork and local requests against what
//! the legacy `plan-v1` op did for the same change. `data/plan_diff/<case>/` holds the request,
//! `expected_outputs` recorded from the engine on a real commit pair (or a real commit with an
//! uncommitted change), and `snapshot/`, the reduced repository the helper plans here: files
//! equal at both commits in `common/`, the others in `base-only/` and `head-only/`.
//! Every file under `snapshot/` and `local/` carries a `.fixture` suffix that the test strips.
//! `record.sh` re-records a case.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::support::fresh_dir;

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it/data/plan_diff")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| unreachable!("{}: {e}", path.display()))
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn copy_tree(from: &Path, to: &Path) {
    let Ok(entries) = fs::read_dir(from) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let target = to.join(name.strip_suffix(".fixture").unwrap_or(&name));
        if entry.path().is_dir() {
            fs::create_dir_all(&target).unwrap_or_else(|e| unreachable!("{e}"));
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap_or_else(|e| unreachable!("{e}"));
        }
    }
}

fn collect_files(dir: &Path, root: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if entry.path().is_dir() {
            collect_files(&entry.path(), root, found);
        } else if let Ok(relative) = entry.path().strip_prefix(root) {
            found.push(relative.to_path_buf());
        }
    }
}

/// The case's repository: a base commit and a head commit for a `pull_request` or fork,
/// one commit for a local run. Returns the repository and the commits' ids.
fn repository(case: &Path, event: &str, name: &str) -> (PathBuf, String, String) {
    let repo = fresh_dir("velnor-ci-plan-diff", name);
    git(&repo, &["init", "-q", "-b", "main"]);
    let snapshot = case.join("snapshot");
    copy_tree(&snapshot.join("common"), &repo);
    copy_tree(&snapshot.join("base-only"), &repo);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "base", "--allow-empty"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    if event != "local" {
        let base_only = snapshot.join("base-only");
        let mut stale = Vec::new();
        collect_files(&base_only, &base_only, &mut stale);
        for file in stale {
            let path = file
                .to_string_lossy()
                .trim_end_matches(".fixture")
                .to_owned();
            fs::remove_file(repo.join(path)).unwrap_or_else(|e| unreachable!("{e}"));
        }
        copy_tree(&snapshot.join("head-only"), &repo);
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "head"]);
    }
    let head = git(&repo, &["rev-parse", "HEAD"]);
    (repo, base, head)
}

/// Where a run's request and outputs go: beside the repository, which they must not dirty.
fn aside(repo: &Path) -> PathBuf {
    let dir = repo.with_extension("io");
    fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    dir
}

fn plan(repo: &Path, request: &Path, outputs: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GITHUB_OUTPUT", outputs)
        .env("GITHUB_RUN_ID", "44491127982")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .args(["plan", "--mode", "ci", "--request"])
        .arg(request)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn bind(request: &str, base: &str, head: &str) -> String {
    let mut value: serde_json::Value =
        serde_json::from_str(request).unwrap_or_else(|e| unreachable!("{e}"));
    if !value["base"].is_null() {
        value["base"] = base.into();
    }
    value["head"] = head.into();
    value.to_string()
}

fn apply_local(case: &Path, repo: &Path) {
    let local = case.join("local");
    let mut appended = Vec::new();
    collect_files(&local.join("append"), &local.join("append"), &mut appended);
    for file in appended {
        let path = file
            .to_string_lossy()
            .trim_end_matches(".fixture")
            .to_owned();
        let mut text = read(&repo.join(&path));
        text.push_str(&read(&local.join("append").join(&file)));
        fs::write(repo.join(&path), text).unwrap_or_else(|e| unreachable!("{e}"));
    }
    copy_tree(&local.join("untracked"), repo);
}

fn event_of(request: &str) -> String {
    serde_json::from_str::<serde_json::Value>(request).unwrap_or_else(|e| unreachable!("{e}"))
        ["event"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn run_case(case: &Path) -> (String, String) {
    let name = case.file_name().and_then(|n| n.to_str()).unwrap_or("case");
    let request = read(&case.join("request.json"));
    let event = event_of(&request);
    let (repo, base, head) = repository(case, &event, name);
    if event == "local" {
        apply_local(case, &repo);
    }
    let file = aside(&repo).join("plan-request.json");
    fs::write(&file, bind(&request, &base, &head)).unwrap_or_else(|e| unreachable!("{e}"));
    let outputs = aside(&repo).join("outputs");
    let output = plan(&repo, &file, &outputs);
    assert!(
        output.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let written = read(&outputs);
    fs::remove_dir_all(&repo).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(aside(&repo)).unwrap_or_else(|e| unreachable!("{e}"));
    (
        written,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn cases() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(data())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

#[test]
fn every_recorded_case_matches_the_legacy_op() {
    let cases = cases();
    assert!(cases.len() >= 4, "{} cases", cases.len());
    for case in &cases {
        let (written, _) = run_case(case);
        assert_eq!(
            written,
            read(&case.join("expected_outputs")),
            "{}",
            case.display()
        );
    }
}

#[test]
fn a_change_to_one_crate_skips_the_jobs_it_does_not_reach() {
    let (written, stderr) = run_case(&data().join("pr-one-crate"));
    let line = written
        .lines()
        .find(|line| line.starts_with("not_affected_jobs="))
        .unwrap_or_default();
    assert!(line.len() > "not_affected_jobs=,,".len(), "{line} {stderr}");
    assert!(!line.contains(",rust-velnor-runner-host,"), "{line}");
    assert!(line.contains(",rust-velnor-gen,"), "{line}");
}

#[test]
fn a_workflow_change_runs_every_job() {
    let (written, stderr) = run_case(&data().join("pr-workflow-change"));
    assert!(written.contains("\nnot_affected_jobs=\n"), "{written}");
    assert!(stderr.contains("global_config_changed"), "{stderr}");
}

fn prepared(name: &str) -> (PathBuf, String) {
    let case = data().join("pr-one-crate");
    let (repo, base, head) = repository(&case, "pull_request", name);
    (repo, bind(&read(&case.join("request.json")), &base, &head))
}

fn planned(request: &str, repo: &Path) -> Output {
    let file = aside(repo).join("plan-request.json");
    fs::write(&file, request).unwrap_or_else(|e| unreachable!("{e}"));
    plan(repo, &file, &aside(repo).join("outputs"))
}

#[test]
fn a_request_without_lane_routes_is_refused() {
    let (repo, request) = prepared("no-routes");
    let mut value: serde_json::Value =
        serde_json::from_str(&request).unwrap_or_else(|e| unreachable!("{e}"));
    value["lane_inventory"] = serde_json::Value::Null;
    let output = planned(&value.to_string(), &repo);
    assert!(!output.status.success());
    assert!(!aside(&repo).join("outputs").exists());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("diff_selection_needs_lane_inventory"),
        "{stderr}"
    );
    fs::remove_dir_all(&repo).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(aside(&repo)).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_checkout_that_is_not_the_head_is_refused() {
    let (repo, request) = prepared("wrong-head");
    let mut value: serde_json::Value =
        serde_json::from_str(&request).unwrap_or_else(|e| unreachable!("{e}"));
    value["head"] = "0".repeat(40).into();
    let output = planned(&value.to_string(), &repo);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("checkout_head_mismatch"), "{stderr}");
    fs::remove_dir_all(&repo).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(aside(&repo)).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn an_untracked_file_runs_every_job() {
    let (repo, request) = prepared("untracked");
    fs::write(repo.join("stray.txt"), "x").unwrap_or_else(|e| unreachable!("{e}"));
    let output = planned(&request, &repo);
    assert!(output.status.success());
    assert!(read(&aside(&repo).join("outputs")).contains("\nnot_affected_jobs=\n"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("untracked_files:all_changed"));
    fs::remove_dir_all(&repo).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(aside(&repo)).unwrap_or_else(|e| unreachable!("{e}"));
}
