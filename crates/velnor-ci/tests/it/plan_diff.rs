// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::plan_diff_repo::{collect_files, repository, repository_with};

pub(crate) fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it/data/plan_diff")
}

pub(crate) fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| unreachable!("{}: {e}", path.display()))
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

pub(crate) fn aside(repo: &Path) -> PathBuf {
    let dir = repo.with_extension("io");
    fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    dir
}

pub(crate) fn plan(repo: &Path, request: &Path, outputs: &Path) -> Output {
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

pub(crate) fn bind(request: &str, base: &str, head: &str) -> String {
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

fn check_tail(id: &str) -> String {
    let quoted = serde_json::Value::from(id).to_string();
    format!(
        "\n[[checks]]\nid = {quoted}\ninputs = [\"zzz-never\"]\nselection = \"declared_inputs\"\n"
    )
}

fn planned_with_check(name: &str, id: &str) -> (PathBuf, Output) {
    let case = data().join("pr-one-crate");
    let (repo, base, head) = repository_with(&case, "pull_request", name, &check_tail(id));
    let request = bind(&read(&case.join("request.json")), &base, &head);
    let output = planned(&request, &repo);
    (repo, output)
}

#[test]
fn a_check_id_with_a_line_break_is_refused_and_adds_no_output_key() {
    let (repo, output) = planned_with_check("injected-id", "x\nqualification_cache_write=true");
    let written = fs::read_to_string(aside(&repo).join("outputs")).unwrap_or_default();
    assert!(!output.status.success(), "{written}");
    assert!(
        !written
            .lines()
            .any(|line| line.starts_with("qualification_cache_write=true")),
        "{written}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("x%0Aqualification_cache_write=true"),
        "{stderr}"
    );
    assert!(!stderr.contains("x\nqualification"), "{stderr}");
    fs::remove_dir_all(&repo).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(aside(&repo)).unwrap_or_else(|e| unreachable!("{e}"));
}

const HOSTILE: [&str; 3] = [
    "x\nother_key=evil",
    "x\n::set-output name=other_key::evil",
    "x\n::add-mask::secret",
];

fn only_known_keys(written: &str) -> bool {
    written.lines().all(|line| {
        ["plan_id", "run_key", "not_affected_jobs", "qualification_"]
            .iter()
            .any(|key| line.starts_with(key))
    })
}

#[test]
fn a_check_id_with_a_line_break_cannot_add_a_plan_output() {
    for (index, id) in HOSTILE.iter().enumerate() {
        let (repo, output) = planned_with_check(&format!("hostile-id-{index}"), id);
        let written = fs::read_to_string(aside(&repo).join("outputs")).unwrap_or_default();
        assert!(only_known_keys(&written), "{id:?}: {written}");
        assert!(!written.contains("evil"), "{id:?}: {written}");
        assert!(!written.contains("secret"), "{id:?}: {written}");
        assert!(!output.status.success(), "{id:?}: {written}");
        fs::remove_dir_all(&repo).unwrap_or_else(|e| unreachable!("{e}"));
        fs::remove_dir_all(aside(&repo)).unwrap_or_else(|e| unreachable!("{e}"));
    }
}

#[test]
fn a_check_id_outside_the_identifier_alphabet_is_refused() {
    let case = data().join("pr-one-crate");
    let (repo, base, head) = repository(&case, "pull_request", "odd-id");
    let request = bind(&read(&case.join("request.json")), &base, &head);
    let config = repo.join(".velnor/config.toml");
    let checked_out = read(&config);
    for id in ["", "a b", "a,b", "a/b", "é"] {
        fs::write(&config, checked_out.clone() + &check_tail(id))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let output = planned(&request, &repo);
        assert!(!output.status.success(), "{id:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("diff_selection_not_supported:check_id"),
            "{stderr}"
        );
    }
    fs::remove_dir_all(&repo).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(aside(&repo)).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_check_id_with_dots_dashes_and_underscores_is_kept() {
    let (repo, output) = planned_with_check("good-id", "jactionlint-linux_x64.1");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let written = read(&aside(&repo).join("outputs"));
    assert!(
        written.contains(",check-jactionlint-linux_x64.1,"),
        "{written}"
    );
    fs::remove_dir_all(&repo).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(aside(&repo)).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_lane_route_id_with_a_line_break_cannot_add_an_output_key() {
    let (repo, request) = prepared("injected-route");
    let mut value: serde_json::Value =
        serde_json::from_str(&request).unwrap_or_else(|e| unreachable!("{e}"));
    let routes = value["lane_inventory"]["routes"]
        .as_array_mut()
        .unwrap_or_else(|| unreachable!("routes"));
    for route in routes.iter_mut().filter(|route| {
        route["logical_job_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("rust-velnor-gen"))
    }) {
        route["job_id"] = "rust-x\nqualification_cache_write=true".into();
    }
    let output = planned(&value.to_string(), &repo);
    let written = fs::read_to_string(aside(&repo).join("outputs")).unwrap_or_default();
    assert!(!output.status.success(), "{written}");
    assert!(
        !written.contains("qualification_cache_write=true"),
        "{written}"
    );
    fs::remove_dir_all(&repo).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(aside(&repo)).unwrap_or_else(|e| unreachable!("{e}"));
}
