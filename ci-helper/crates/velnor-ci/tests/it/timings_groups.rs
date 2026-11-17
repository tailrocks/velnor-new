// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::github::{job_log_path, jobs_path};
use velnor_ci::timings::gather;
use velnor_model::RepoPath;

use crate::support::FakeGitHub;
use crate::timings::{LIST, lab, recorded, roots, runner};

const NEWEST: [u64; 3] = [43_636_016_430, 43_628_937_052, 43_618_131_619];

fn list(entries: &[Value]) -> String {
    json!({ "total_count": entries.len(), "workflow_runs": entries }).to_string()
}

fn entry(id: u64, event: &str, branch: &str) -> Value {
    json!({ "id": id, "event": event, "run_attempt": 1, "head_branch": branch, "pull_requests": [{"base": {"ref": "main"}}] })
}

fn ordinary(ids: &[u64]) -> Vec<Value> {
    ids.iter()
        .map(|id| entry(*id, "pull_request", "topic"))
        .collect()
}

fn root(path: &str) -> RepoPath {
    RepoPath::new(path).unwrap_or_else(|e| unreachable!("{e}"))
}

fn archived_test_jobs() -> FakeGitHub {
    let mut fake = recorded(&list(&ordinary(&NEWEST)));
    let repo = lab();
    for (index, id) in NEWEST.into_iter().enumerate() {
        let path = jobs_path(&repo, id, 1, 1);
        let mut jobs: Value =
            serde_json::from_str(&fake.get_text(&path)).unwrap_or_else(|e| unreachable!("{e}"));
        let job_list = jobs["jobs"]
            .as_array_mut()
            .unwrap_or_else(|| unreachable!());
        let at = job_list
            .iter()
            .position(|job| job["name"] == "rust-root")
            .unwrap_or_else(|| unreachable!());
        let template = job_list[at].clone();
        let lint = template["steps"]
            .as_array()
            .and_then(|steps| steps.iter().find(|step| step["name"] == "Lint"))
            .cloned()
            .unwrap_or_else(|| unreachable!());
        let mut lint_job = template.clone();
        lint_job["name"] = "rust-lint".into();
        lint_job["steps"] = json!([lint]);
        job_list[at] = lint_job;
        let lint_id = template["id"].as_u64().unwrap_or_default();
        fake.respond(
            job_log_path(&repo, lint_id),
            "##[group]Run cargo-clippy clippy --workspace\n",
        );
        let build_seconds = [7, 9, 11][index];
        let test_seconds = [3, 5, 7][index];
        for (job_id, name, step, log) in [
            (
                10_000 + index as u64,
                "rust-test",
                "Build the tests",
                format!(
                    "##[group]Run cargo-nextest nextest archive\nFinished `test` profile [unoptimized + debuginfo] target(s) in {build_seconds}.00s\n"
                ),
            ),
            (
                11_000 + index as u64,
                "rust-test-default",
                "Test",
                format!(
                    "##[group]Run cargo-nextest nextest run\nPASS [ {test_seconds}.00s] (1/1) lab::bin/lab\nSummary [ {test_seconds}.00s] 1 test run: 1 passed, 0 skipped\n"
                ),
            ),
        ] {
            let mut job = template.clone();
            job["id"] = job_id.into();
            job["name"] = name.into();
            job["started_at"] = "2026-11-14T20:10:22Z".into();
            job["completed_at"] = "2026-11-14T20:10:45Z".into();
            job["steps"] = json!([{
                "name": step,
                "conclusion": "success",
                "started_at": "2026-11-14T20:10:22Z",
                "completed_at": format!("2026-11-14T20:10:{:02}Z", 22 + build_seconds),
            }]);
            jobs["jobs"]
                .as_array_mut()
                .unwrap_or_else(|| unreachable!())
                .push(job);
            fake.respond(job_log_path(&repo, job_id), &log);
        }
        fake.respond(path, &jobs.to_string());
    }
    fake
}

#[test]
fn grouped_test_timings_use_the_archive_build_and_default_group_execution_jobs() {
    let report = gather(
        &archived_test_jobs(),
        &lab(),
        &[root("Cargo.toml")],
        &runner(),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let timing = &report.timings[&root("Cargo.toml")];
    assert_eq!(timing.test_build_seconds, 9);
    assert_eq!(timing.test_seconds, 5);
    assert_eq!(timing.test_binary_seconds["lab::bin/lab"], 5);

    let dir =
        std::env::temp_dir().join(format!("velnor-ci-timings-grouped-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    velnor_ci::io::write_report(&dir, report).unwrap_or_else(|e| unreachable!("{e}"));
    let file = std::fs::read_to_string(dir.join(".velnor/rust-ci-timings.json"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::remove_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    let written: Value = serde_json::from_str(&file).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(written["Cargo.toml"]["test_build_seconds"], 9);
    assert_eq!(written["Cargo.toml"]["test_seconds"], 5);
    assert_eq!(
        written["Cargo.toml"]["test_binary_seconds"]["lab::bin/lab"],
        5
    );
}

fn lint_measurement_fake(invalid: &str) -> FakeGitHub {
    let mut fake = recorded(LIST);
    for (index, id) in NEWEST.into_iter().enumerate() {
        let path = jobs_path(&lab(), id, 1, 1);
        let mut jobs: Value =
            serde_json::from_str(&fake.get_text(&path)).unwrap_or_else(|e| unreachable!("{e}"));
        let root_job = jobs["jobs"]
            .as_array_mut()
            .and_then(|jobs| jobs.iter_mut().find(|job| job["name"] == "rust-root"))
            .unwrap_or_else(|| unreachable!());
        let step = root_job["steps"]
            .as_array_mut()
            .and_then(|steps| steps.iter_mut().find(|step| step["name"] == "Lint"))
            .unwrap_or_else(|| unreachable!());
        match (index, invalid) {
            (0, "skipped") => {
                step["conclusion"] = "skipped".into();
                let log_path = job_log_path(&lab(), root_job["id"].as_u64().unwrap_or_default());
                let log = fake.get_text(&log_path);
                let kept = velnor_ci::logscan::split_steps(&log)
                    .into_iter()
                    .filter(|part| !part.contains("##[group]Run cargo-clippy"))
                    .collect::<String>();
                fake.respond(log_path, &kept);
            }
            (1, "missing") => step["started_at"] = Value::Null,
            (2, "malformed") => step["started_at"] = "not-a-timestamp".into(),
            (2, "backwards") => step["completed_at"] = "2026-11-14T00:00:00Z".into(),
            (index, "zero") => {
                let equal = format!("2026-11-14T20:10:{:02}Z", 18 + index);
                step["started_at"] = equal.clone().into();
                step["completed_at"] = equal.into();
            }
            _ => {}
        }
        fake.respond(path, &jobs.to_string());
    }
    fake
}

#[test]
fn unmeasured_lint_steps_do_not_create_samples_but_a_measured_zero_does() {
    for invalid in ["skipped", "missing", "malformed", "backwards"] {
        let report = gather(&lint_measurement_fake(invalid), &lab(), &roots(), &runner())
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(
            !report.timings.contains_key(&root("Cargo.toml")),
            "{invalid}"
        );
    }
    let zero = gather(&lint_measurement_fake("zero"), &lab(), &roots(), &runner())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        zero.timings.contains_key(&root("Cargo.toml")),
        "{:?}",
        zero.notes
    );
    assert_eq!(zero.timings[&root("Cargo.toml")].lint_build_seconds, 0);
}

#[test]
fn recorded_pull_requests_without_base_metadata_are_not_written_as_default_branch_timings() {
    let report = gather(&recorded(LIST), &lab(), &roots(), &runner())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.timings.is_empty());

    let dir =
        std::env::temp_dir().join(format!("velnor-ci-timings-empty-pr-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    let message = velnor_ci::io::write_report(&dir, report).unwrap_or_default();
    let exists = dir.join(".velnor/rust-ci-timings.json").exists();
    std::fs::remove_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(message.contains("was not written"), "{message}");
    assert!(!exists);

    let default_base = gather(
        &recorded(&list(&ordinary(&NEWEST))),
        &lab(),
        &roots(),
        &runner(),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(default_base.timings.len(), 2);
}
