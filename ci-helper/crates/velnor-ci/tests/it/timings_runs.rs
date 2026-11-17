// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::collect::collect;
use velnor_ci::github::{job_log_path, jobs_path, run_path, workflow_runs_path};
use velnor_ci::timings::gather;
use velnor_model::RepoPath;

use crate::support::FakeGitHub;
use crate::timings::{LIST, lab, median, recorded, roots, runner};

const NEWEST: [u64; 3] = [43_636_016_430, 43_628_937_052, 43_618_131_619];

fn list(entries: &[Value]) -> String {
    json!({ "total_count": entries.len(), "workflow_runs": entries }).to_string()
}

fn entry(id: u64, event: &str, branch: &str) -> Value {
    json!({ "id": id, "event": event, "run_attempt": 1, "head_branch": branch, "pull_requests": [{"base": {"ref": "main"}}] })
}

fn empty_pull_request(id: u64) -> Value {
    let mut value = entry(id, "pull_request", "topic");
    value["pull_requests"] = json!([]);
    value
}

fn ordinary(ids: &[u64]) -> Vec<Value> {
    ids.iter()
        .map(|id| entry(*id, "pull_request", "topic"))
        .collect()
}

fn root(path: &str) -> RepoPath {
    RepoPath::new(path).unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn an_even_number_of_runs_takes_the_mean_of_the_middle_two() {
    let ids = [NEWEST[0], NEWEST[1], NEWEST[2], NEWEST[0]];
    let fake = recorded(&list(&ordinary(&ids)));
    let report = gather(&fake, &lab(), &roots(), &runner()).unwrap_or_else(|e| unreachable!("{e}"));
    let runs: Vec<_> = ids
        .iter()
        .map(|id| collect(&fake, &lab(), *id, false).unwrap_or_else(|e| unreachable!("{e}")))
        .collect();
    let lint = |name: &str| {
        runs.iter()
            .map(|run| {
                let job = run.jobs.iter().find(|j| j.name == name);
                let step = job.and_then(|j| j.steps.iter().find(|s| s.name == "Lint"));
                f64::from(
                    u32::try_from(step.and_then(|s| s.seconds).unwrap_or_default())
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        report.timings[&root("lab/Cargo.toml")].lint_build_seconds,
        median(lint("rust-lab"))
    );
    assert_eq!(
        report.timings[&root("Cargo.toml")].lint_build_seconds,
        median(lint("rust-root"))
    );
    assert_ne!(lint("rust-lab")[0], lint("rust-lab")[1]);
}

fn split_lab_jobs(lint_conclusion: &str) -> FakeGitHub {
    let mut fake = recorded(&list(&ordinary(&NEWEST)));
    for (index, id) in NEWEST.iter().enumerate() {
        let path = jobs_path(&lab(), *id, 1, 1);
        let mut jobs: Value =
            serde_json::from_str(&fake.get_text(&path)).unwrap_or_else(|e| unreachable!("{e}"));
        let list = jobs["jobs"]
            .as_array_mut()
            .unwrap_or_else(|| unreachable!());
        let at = list
            .iter()
            .position(|j| j["name"] == "rust-lab")
            .unwrap_or_else(|| unreachable!());
        let mut first_job = list[at].clone();
        first_job["id"] = (9_000 + index).into();
        first_job["name"] = "rust-lint-lab".into();
        first_job["conclusion"] = lint_conclusion.into();
        if lint_conclusion == "skipped" {
            first_job["started_at"] = Value::Null;
        }
        list[at]["name"] = "rust-test-lab".into();
        list.push(first_job);
        fake.respond(path, &jobs.to_string());
        let log = velnor_ci::github::job_log_path(&lab(), 9_000 + index as u64);
        fake.respond(log, "");
    }
    fake
}

#[test]
fn lint_and_test_jobs_make_one_entry_unless_the_lint_job_did_not_run() {
    let ran = gather(&split_lab_jobs("success"), &lab(), &roots(), &runner())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(ran.timings.contains_key(&root("lab/Cargo.toml")));
    let skipped = gather(&split_lab_jobs("skipped"), &lab(), &roots(), &runner())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!skipped.timings.contains_key(&root("lab/Cargo.toml")));
    assert!(skipped.timings.contains_key(&root("Cargo.toml")));
}

#[test]
fn a_second_page_is_read_and_ten_runs_are_the_most() {
    let mut first: Vec<Value> = (0..98)
        .map(|n| entry(1_000 + n, "workflow_dispatch", "topic"))
        .collect();
    first.extend(ordinary(&NEWEST[..2]));
    let mut second = ordinary(&NEWEST[2..]);
    second.extend(ordinary(&[NEWEST[0]; 7]));
    second.extend(ordinary(&[1, 2, 3, 4, 5]));
    let mut fake = recorded(&list(&first));
    fake.respond(workflow_runs_path(&lab(), "rust-ci.yml", 2), &list(&second));
    let report = gather(&fake, &lab(), &roots(), &runner()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(report.timings.len(), 2);
    assert_eq!(report.notes, Vec::<String>::new());
}

#[test]
fn a_push_counts_only_on_the_default_branch() {
    let pushes = |branch: &str| -> String {
        list(
            &NEWEST
                .iter()
                .map(|id| entry(*id, "push", branch))
                .collect::<Vec<_>>(),
        )
    };
    let on_main = gather(&recorded(&pushes("main")), &lab(), &roots(), &runner())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(on_main.timings.len(), 2);
    let elsewhere = gather(&recorded(&pushes("topic")), &lab(), &roots(), &runner())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(elsewhere.timings.is_empty());
}

#[test]
fn the_file_written_is_the_one_generate_reads() {
    let report = gather(&recorded(LIST), &lab(), &roots(), &runner())
        .unwrap_or_else(|e| unreachable!("{e}"));
    let dir = std::env::temp_dir().join(format!("velnor-ci-timings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    let message = velnor_ci::io::write_report(&dir, report.clone());
    let file = dir.join(".velnor/rust-ci-timings.json");
    let text = std::fs::read_to_string(&file);
    std::fs::remove_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        message.unwrap_or_default(),
        "wrote .velnor/rust-ci-timings.json for 2 Cargo roots\n"
    );
    assert_eq!(
        text.unwrap_or_default(),
        velnor_rust::render_timings(&report.timings)
    );
}

#[test]
fn nothing_is_written_when_there_is_nothing_to_write() {
    let dir = std::env::temp_dir().join(format!("velnor-ci-timings-none-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    let none = velnor_ci::timings::TimingsReport {
        timings: std::collections::BTreeMap::new(),
        notes: vec!["too few".to_owned()],
    };
    let message = velnor_ci::io::write_report(&dir, none).unwrap_or_default();
    let exists = dir.join(".velnor").exists();
    std::fs::remove_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        message,
        "too few\n.velnor/rust-ci-timings.json was not written\n"
    );
    assert!(!exists);
}

#[test]
fn pages_are_read_until_enough_ordinary_runs_are_found() {
    let mut fake = recorded(&list(&[]));
    for page in 1..=6_u32 {
        let manual: Vec<Value> = (0..100)
            .map(|n| entry(10_000 + n, "workflow_dispatch", "topic"))
            .collect();
        fake.respond(
            workflow_runs_path(&lab(), "rust-ci.yml", page),
            &list(&manual),
        );
    }
    fake.respond(
        workflow_runs_path(&lab(), "rust-ci.yml", 7),
        &list(&ordinary(&NEWEST)),
    );
    let report = gather(&fake, &lab(), &roots(), &runner()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(report.timings.len(), 2);
}

#[test]
fn a_binary_seen_in_fewer_than_three_runs_has_no_entry() {
    let mut fake = recorded(&list(&ordinary(&NEWEST)));
    let log = velnor_ci::github::job_log_path(&lab(), 132_517_592_524);
    let kept: Vec<String> = fake
        .get_text(&log)
        .lines()
        .filter(|line| !line.contains("lab::bin/lab"))
        .map(str::to_owned)
        .collect();
    let without = kept.join("\n");
    fake.respond(log, &without);
    let report = gather(&fake, &lab(), &roots(), &runner()).unwrap_or_else(|e| unreachable!("{e}"));
    let lab_entry = &report.timings[&root("lab/Cargo.toml")];
    assert!(lab_entry.test_binary_seconds.is_empty());
    assert!(
        !report.timings[&root("Cargo.toml")]
            .test_binary_seconds
            .is_empty()
    );
}

#[test]
fn a_run_whose_logs_were_deleted_is_skipped() {
    let extra = NEWEST[0] + 1;
    let mut fake = recorded(&list(&ordinary(&[extra, NEWEST[0], NEWEST[1], NEWEST[2]])));
    let run = fake.get_text(&run_path(&lab(), NEWEST[0]));
    let mut jobs: Value = serde_json::from_str(&fake.get_text(&jobs_path(&lab(), NEWEST[0], 1, 1)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    for job in jobs["jobs"].as_array_mut().into_iter().flatten() {
        let id = job["id"].as_u64().unwrap_or_default() + 1_000_000;
        job["id"] = id.into();
        fake.missing(job_log_path(&lab(), id));
    }
    fake.respond(run_path(&lab(), extra), &run);
    fake.respond(jobs_path(&lab(), extra, 1, 1), &jobs.to_string());
    let report = gather(&fake, &lab(), &roots(), &runner()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(report.timings.len(), 2);
}

#[test]
fn a_failure_that_is_not_a_missing_log_still_stops_the_command() {
    let extra = NEWEST[0] + 1;
    let mut fake = recorded(&list(&ordinary(&[extra, NEWEST[0], NEWEST[1], NEWEST[2]])));
    let run = fake.get_text(&run_path(&lab(), NEWEST[0]));
    fake.respond(run_path(&lab(), extra), &run);
    assert!(gather(&fake, &lab(), &roots(), &runner()).is_err());
}

const SLOW_RUNS: [u64; 3] = [7_001, 7_002, 7_003];

fn on_slow_runner(fake: &mut FakeGitHub) {
    let repo = lab();
    let mut value: Value = serde_json::from_str(&fake.get_text(&jobs_path(&repo, NEWEST[0], 1, 1)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    for job in value["jobs"].as_array_mut().into_iter().flatten() {
        job["labels"] = json!(["slow-box"]);
        for step in job["steps"].as_array_mut().into_iter().flatten() {
            if step["name"] == "Lint" {
                step["completed_at"] = json!("2026-11-14T23:00:00Z");
            }
        }
    }
    let run = fake.get_text(&run_path(&repo, NEWEST[0]));
    for id in SLOW_RUNS {
        fake.respond(run_path(&repo, id), &run);
        fake.respond(jobs_path(&repo, id, 1, 1), &value.to_string());
    }
}

fn two_runners() -> FakeGitHub {
    let mut fake = recorded(&list(&ordinary(
        &[NEWEST.as_slice(), SLOW_RUNS.as_slice()].concat(),
    )));
    on_slow_runner(&mut fake);
    fake
}

#[test]
fn only_runs_on_the_configured_runner_count() {
    let fake = two_runners();
    let ubuntu = gather(&fake, &lab(), &roots(), &runner()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(ubuntu.notes, Vec::<String>::new());
    assert_eq!(ubuntu.timings, crate::timings::report(LIST).timings);
}

#[test]
fn a_runner_with_three_runs_gets_entries() {
    let fake = two_runners();
    let slow = velnor_model::RunnerLabel::from_static("slow-box");
    let report = gather(&fake, &lab(), &roots(), &slow).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(report.timings.len(), 2);
    assert!(report.timings[&root("Cargo.toml")].lint_build_seconds > 1_000);
}

fn archived_test_jobs() -> FakeGitHub {
    let mut fake = recorded(&list(&ordinary(&NEWEST)));
    let repo = lab();
    for (index, id) in NEWEST.into_iter().enumerate() {
        let path = jobs_path(&repo, id, 1, 1);
        let mut jobs: Value =
            serde_json::from_str(&fake.get_text(&path)).unwrap_or_else(|e| unreachable!("{e}"));
        let template = jobs["jobs"]
            .as_array()
            .and_then(|jobs| jobs.iter().find(|job| job["name"] == "rust-root"))
            .cloned()
            .unwrap_or_else(|| unreachable!());
        let build_seconds = [7, 9, 11][index];
        let test_seconds = [3, 5, 7][index];
        for (job_id, name, step, log) in [
            (
                10_000 + index as u64,
                "rust-test-root",
                "Build the tests",
                format!(
                    "##[group]Run cargo-nextest nextest archive\nFinished `test` profile [unoptimized + debuginfo] target(s) in {build_seconds}.00s\n"
                ),
            ),
            (
                11_000 + index as u64,
                "rust-test-default-root",
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
    let report = gather(&archived_test_jobs(), &lab(), &roots(), &runner())
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
fn pull_requests_without_base_metadata_are_not_written_as_default_branch_timings() {
    let empty = list(&NEWEST.map(empty_pull_request));
    let report = gather(&recorded(&empty), &lab(), &roots(), &runner())
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

#[test]
fn a_runner_seen_in_no_run_gets_no_entry() {
    let fake = two_runners();
    let other = velnor_model::RunnerLabel::from_static("arm-box");
    let report = gather(&fake, &lab(), &roots(), &other).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.timings.is_empty());
    assert!(
        report.notes[0].contains("0 of 6 runs on `arm-box`"),
        "{:?}",
        report.notes
    );
}

fn relabel(fake: &mut FakeGitHub, labels: Option<&Value>) {
    for id in NEWEST {
        let path = jobs_path(&lab(), id, 1, 1);
        let mut value: Value =
            serde_json::from_str(&fake.get_text(&path)).unwrap_or_else(|e| unreachable!("{e}"));
        for job in value["jobs"].as_array_mut().into_iter().flatten() {
            match labels {
                Some(labels) => job["labels"] = labels.clone(),
                None => {
                    if let Some(object) = job.as_object_mut() {
                        object.remove("labels");
                    }
                }
            }
        }
        fake.respond(path, &value.to_string());
    }
}

#[test]
fn a_label_differing_only_in_case_counts_for_the_runner() {
    let mut fake = recorded(LIST);
    relabel(&mut fake, Some(&json!(["Ubuntu-Latest"])));
    let report = gather(&fake, &lab(), &roots(), &runner()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(report.notes, Vec::<String>::new());
    assert_eq!(report.timings, crate::timings::report(LIST).timings);
}

#[test]
fn a_job_with_no_labels_counts_for_no_runner() {
    for labels in [Some(json!([])), None] {
        let mut fake = recorded(LIST);
        relabel(&mut fake, labels.as_ref());
        let report =
            gather(&fake, &lab(), &roots(), &runner()).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(report.timings.is_empty());
        assert_eq!(report.notes.len(), 2);
        assert!(
            report.notes[0].contains("0 of 3 runs on `ubuntu-latest`"),
            "{:?}",
            report.notes
        );
    }
}
