// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! `velnor-ci plan --mode ... --request` against what the legacy `plan-v1` op did for the
//! same request. `data/plan_modes/<case>/` holds the request and the recorded result
//! (`expected_outputs`, or `expected_error`); `record.sh` re-records it from the engine.
//! The two qualification cases that pass every check are not recorded: the engine then
//! refuses them with the code in `legacy_refusal`, because the named check entries of this
//! repository carry no cache ids. Their `helper_outputs` come from the phase table of the
//! engine's `QualificationPhase` (the cache is on unless the phase is `control`, and a plan
//! never writes it).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use std::time::Duration;

use crate::support::{fresh_dir, make_fifo, output_within};

const DEFAULT_BRANCH: &str = "main";

fn cases() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it/data/plan_modes");
    let mut dirs: Vec<PathBuf> = fs::read_dir(root)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| unreachable!("{}: {e}", path.display()))
}

fn mode_of(request: &str) -> &'static str {
    let value: serde_json::Value =
        serde_json::from_str(request).unwrap_or_else(|e| unreachable!("{e}"));
    if value["event"] == "qualification" {
        "qualification"
    } else {
        "ci"
    }
}

fn head_of(request: &Path) -> String {
    let value: serde_json::Value =
        serde_json::from_str(&read(request)).unwrap_or(serde_json::Value::Null);
    value["head"].as_str().unwrap_or_default().to_owned()
}

fn plan(request: &Path, mode: &str, outputs: &Path, extra: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .env_clear()
        .env("GITHUB_SHA", head_of(request))
        .env("GITHUB_OUTPUT", outputs)
        .env("GITHUB_RUN_ID", "44491127982")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .args(["plan", "--mode", mode, "--default-branch", DEFAULT_BRANCH])
        .arg("--request")
        .arg(request);
    for (name, value) in extra {
        command.env(name, value);
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn every_recorded_case_matches_the_legacy_op() {
    let cases = cases();
    assert!(cases.len() >= 14, "{} cases", cases.len());
    for case in &cases {
        let name = case.file_name().and_then(|n| n.to_str()).unwrap_or("case");
        let temp = fresh_dir("velnor-ci-plan-modes", name);
        let outputs = temp.join("outputs");
        let request = case.join("request.json");
        let output = plan(&request, mode_of(&read(&request)), &outputs, &[]);
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let expected = ["expected_outputs", "helper_outputs"]
            .into_iter()
            .map(|file| case.join(file))
            .find(|path| path.is_file());
        if let Some(expected) = expected {
            assert!(output.status.success(), "{name}: {stderr}");
            assert_eq!(read(&outputs), read(&expected), "{name}");
        } else {
            let code = read(&case.join("expected_error"));
            assert!(!output.status.success(), "{name} planned");
            assert!(
                stderr
                    .trim_end()
                    .ends_with(&format!("internal request failed: {code}")),
                "{name}: expected {code}, got {stderr}"
            );
            assert!(!outputs.exists(), "{name} wrote outputs");
        }
        fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    }
}

fn refusal(request: &str, mode: &str, extra: &[(&str, &str)]) -> String {
    let temp = fresh_dir("velnor-ci-plan-modes", "refusal");
    let file = temp.join("request.json");
    fs::write(&file, request).unwrap_or_else(|e| unreachable!("{e}"));
    let output = plan(&file, mode, &temp.join("outputs"), extra);
    assert!(!output.status.success(), "planned: {request}");
    assert!(!temp.join("outputs").exists());
    fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn recorded(case: &str) -> String {
    read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/it/data/plan_modes")
            .join(case)
            .join("request.json"),
    )
}

#[test]
fn a_phase_with_a_predecessor_is_refused_rather_than_planned_without_its_cache_directive() {
    let request = recorded("qualification-cold").replace(
        "\"phase\":\"cold\"",
        "\"phase\":\"warm\",\"predecessor\":{\"run_attempt\":1,\"run_id\":5}",
    );
    let stderr = refusal(&request, "qualification", &[]);
    assert!(
        stderr.contains("qualification_phase_not_supported:warm"),
        "{stderr}"
    );
}

#[test]
fn the_mode_must_match_the_event() {
    let stderr = refusal(&recorded("qualification-cold"), "ci", &[]);
    assert!(
        stderr.contains("qualification_event_needs_qualification_mode"),
        "{stderr}"
    );
    let stderr = refusal(&recorded("push-rust-only"), "qualification", &[]);
    assert!(
        stderr.contains("qualification_mode_needs_qualification_event"),
        "{stderr}"
    );
}

#[test]
fn the_run_key_follows_the_attempt_override_then_the_request() {
    let temp = fresh_dir("velnor-ci-plan-modes", "run-key");
    let outputs = temp.join("outputs");
    let request = temp.join("request.json");
    fs::write(&request, recorded("push-docs-only")).unwrap_or_else(|e| unreachable!("{e}"));
    let output = plan(&request, "ci", &outputs, &[("VELNOR_RUN_KEY_ATTEMPT", "7")]);
    assert!(output.status.success());
    assert!(read(&outputs).starts_with("plan_id=plan-r44491127982-a7\nrun_key=r44491127982-a7\n"));
    fs::remove_file(&outputs).unwrap_or_else(|e| unreachable!("{e}"));
    let keyed = recorded("push-docs-only").replace("\"op\"", "\"run_key\":\"r9-a2\",\"op\"");
    fs::write(&request, keyed).unwrap_or_else(|e| unreachable!("{e}"));
    let output = plan(&request, "ci", &outputs, &[("VELNOR_RUN_KEY_ATTEMPT", "7")]);
    assert!(output.status.success());
    assert!(read(&outputs).starts_with("plan_id=plan-r9-a2\nrun_key=r9-a2\n"));
    fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_qualification_run_needs_the_default_branch() {
    let temp = fresh_dir("velnor-ci-plan-modes", "branch");
    let request = temp.join("request.json");
    fs::write(&request, recorded("qualification-cold")).unwrap_or_else(|e| unreachable!("{e}"));
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .env_clear()
        .env("GITHUB_RUN_ID", "44491127982")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .args(["plan", "--mode", "qualification", "--request"])
        .arg(&request)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing_default_branch"));
    fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_duplicate_request_key_is_refused_before_planning() {
    let request = recorded("push-docs-only").replacen('{', "{\"event\":\"merge_group\",", 1);
    let stderr = refusal(&request, "ci", &[]);
    assert!(stderr.contains("duplicate_key:event"), "{stderr}");
}

#[test]
fn every_accepted_request_field_is_type_checked() {
    for (field, value) in [
        ("root", serde_json::json!(42)),
        ("named_check_lanes", serde_json::json!("bad")),
        ("lane_inventory", serde_json::json!("bad")),
        (
            "lane_inventory",
            serde_json::json!({"routes": [], "extra": 1}),
        ),
    ] {
        let mut request: serde_json::Value = serde_json::from_str(&recorded("push-docs-only"))
            .unwrap_or_else(|e| unreachable!("{e}"));
        request[field] = value;
        let request = request.to_string();
        let stderr = refusal(&request, "ci", &[]);
        assert!(stderr.contains("malformed_request"), "{field}: {stderr}");
    }
}

#[test]
fn a_qualification_head_must_be_the_sha_of_the_current_run() {
    let other = "1".repeat(40);
    for sha in [other.as_str(), ""] {
        let stderr = refusal(
            &recorded("qualification-cold"),
            "qualification",
            &[("GITHUB_SHA", sha)],
        );
        assert!(stderr.contains("source_sha_not_current_run"), "{stderr}");
    }
}

#[test]
fn a_request_that_is_a_fifo_is_refused_instead_of_waiting_for_a_writer() {
    let temp = fresh_dir("velnor-ci-plan-modes", "fifo");
    let fifo = temp.join("request.json");
    make_fifo(&fifo);
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .env_clear()
        .env("GITHUB_OUTPUT", temp.join("outputs"))
        .args(["plan", "--mode", "ci", "--request"])
        .arg(&fifo);
    let output = output_within(&mut command, Duration::from_secs(20));
    let stderr = output.map(|output| String::from_utf8_lossy(&output.stderr).into_owned());
    assert!(
        stderr
            .as_deref()
            .is_some_and(|text| text.contains("unreadable_request")),
        "{stderr:?}"
    );
    fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
}
