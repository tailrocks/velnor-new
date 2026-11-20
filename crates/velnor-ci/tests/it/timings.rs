// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::collect::collect;
use velnor_ci::github::{Repo, job_log_path, jobs_path, repo_path, run_path, workflow_runs_path};
use velnor_ci::timings::{TimingsReport, gather};
use velnor_model::{RepoPath, RunnerLabel};

use crate::support::FakeGitHub;

pub(crate) const LIST: &str = include_str!("data/timings/list-main-pr.json");
pub(crate) const RECORDED_LIST: &str = include_str!("data/timings/list.json");
const REPO: &str = include_str!("data/timings/repo.json");

const RUNS: &[(u64, &str, &str)] = &[
    (
        43_636_016_430,
        include_str!("data/timings/run-43636016430.json"),
        include_str!("data/timings/jobs-43636016430.json"),
    ),
    (
        43_628_937_052,
        include_str!("data/timings/run-43628937052.json"),
        include_str!("data/timings/jobs-43628937052.json"),
    ),
    (
        43_618_131_619,
        include_str!("data/timings/run-43618131619.json"),
        include_str!("data/timings/jobs-43618131619.json"),
    ),
];

const LOGS: &[(u64, &str)] = &[
    (
        132_572_876_594,
        include_str!("data/timings/log-132572876594.log"),
    ),
    (
        132_572_876_877,
        include_str!("data/timings/log-132572876877.log"),
    ),
    (
        132_550_990_828,
        include_str!("data/timings/log-132550990828.log"),
    ),
    (
        132_550_991_137,
        include_str!("data/timings/log-132550991137.log"),
    ),
    (
        132_517_592_499,
        include_str!("data/timings/log-132517592499.log"),
    ),
    (
        132_517_592_524,
        include_str!("data/timings/log-132517592524.log"),
    ),
    (
        132_517_822_198,
        include_str!("data/timings/log-132517822198.log"),
    ),
    (
        132_551_215_452,
        include_str!("data/timings/log-132551215452.log"),
    ),
    (
        132_573_124_848,
        include_str!("data/timings/log-132573124848.log"),
    ),
];

pub(crate) fn lab() -> Repo {
    Repo::parse("tailrocks/velnor-lab").unwrap_or_else(|e| unreachable!("{e}"))
}

pub(crate) fn runner() -> RunnerLabel {
    RunnerLabel::from_static("ubuntu-latest")
}

pub(crate) fn roots() -> Vec<RepoPath> {
    ["Cargo.toml", "lab/Cargo.toml"]
        .map(|path| RepoPath::new(path).unwrap_or_else(|e| unreachable!("{e}")))
        .to_vec()
}

pub(crate) fn recorded(list: &str) -> FakeGitHub {
    let repo = lab();
    let mut fake = FakeGitHub::default();
    fake.respond(repo_path(&repo), REPO);
    fake.respond(workflow_runs_path(&repo, "rust-ci.yml", 1), list);
    for &(id, run, jobs) in RUNS {
        fake.respond(run_path(&repo, id), run);
        fake.respond(jobs_path(&repo, id, 1, 1), jobs);
    }
    for &(id, log) in LOGS {
        fake.respond(job_log_path(&repo, id), log);
    }
    fake
}

pub(crate) fn report(list: &str) -> TimingsReport {
    gather(&recorded(list), &lab(), &roots(), &runner()).unwrap_or_else(|e| unreachable!("{e}"))
}

pub(crate) fn median(mut values: Vec<f64>) -> u64 {
    values.sort_by(f64::total_cmp);
    let (low, high) = (values[(values.len() - 1) / 2], values[values.len() / 2]);
    let seconds = std::time::Duration::from_secs_f64(f64::midpoint(low, high).round());
    seconds.as_secs()
}

#[test]
fn the_file_holds_the_medians_of_the_step_times_bench_collect_reports() {
    let report = report(LIST);
    assert_eq!(report.notes, Vec::<String>::new());
    let runs: Vec<_> = RUNS
        .iter()
        .map(|(id, _, _)| {
            collect(&recorded(LIST), &lab(), *id, false).unwrap_or_else(|e| unreachable!("{e}"))
        })
        .collect();
    for (root, job) in [("Cargo.toml", "rust-root"), ("lab/Cargo.toml", "rust-lab")] {
        let step = |run: &velnor_ci::metrics::RunMetrics, name: &str| {
            let job = run
                .jobs
                .iter()
                .find(|j| j.name == job)
                .unwrap_or_else(|| unreachable!("{job}"));
            job.steps
                .iter()
                .find(|s| s.name == name)
                .cloned()
                .unwrap_or_else(|| unreachable!("{name}"))
        };
        let got = &report.timings[&RepoPath::new(root).unwrap_or_else(|e| unreachable!("{e}"))];
        assert_eq!(
            got.lint_build_seconds,
            median(
                runs.iter()
                    .map(|r| f64::from(
                        u32::try_from(step(r, "Lint").seconds.unwrap_or_default())
                            .unwrap_or_default()
                    ))
                    .collect()
            )
        );
        assert_eq!(
            got.test_build_seconds,
            median(
                runs.iter()
                    .map(|r| step(r, "Test").log.finished_seconds[0])
                    .collect()
            )
        );
        assert_eq!(
            got.test_seconds,
            median(
                runs.iter()
                    .map(|r| step(r, "Test").log.nextest[0].seconds)
                    .collect()
            )
        );
    }
}

#[test]
fn recorded_medians_and_binaries_are_exact() {
    let report = report(LIST);
    let root =
        &report.timings[&RepoPath::new("Cargo.toml").unwrap_or_else(|e| unreachable!("{e}"))];
    assert_eq!(root.lint_build_seconds, 4);
    assert_eq!(root.test_build_seconds, 3);
    assert_eq!(root.test_seconds, 20);
    let lab =
        &report.timings[&RepoPath::new("lab/Cargo.toml").unwrap_or_else(|e| unreachable!("{e}"))];
    assert_eq!(lab.test_build_seconds, 9);
    assert_eq!(lab.test_binary_seconds.len(), 1);
    assert_eq!(lab.test_binary_seconds["lab::bin/lab"], 0);
}

#[test]
fn only_ordinary_runs_count() {
    let report = report(LIST);
    assert_eq!(report.timings.len(), 2);
    let manual_only = LIST.replace("\"pull_request\"", "\"workflow_dispatch\"");
    let report = gather(&recorded(&manual_only), &lab(), &roots(), &runner())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.timings.is_empty());
    assert!(report.notes[0].contains("0 ordinary"), "{:?}", report.notes);
    let reruns = LIST.replace("\"run_attempt\":1", "\"run_attempt\":2");
    let report = gather(&recorded(&reruns), &lab(), &roots(), &runner())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.timings.is_empty());
}

#[test]
fn a_pull_request_into_another_branch_is_not_the_default_branch() {
    let elsewhere = LIST.replace("\"ref\":\"main\"", "\"ref\":\"release\"");
    let report = gather(&recorded(&elsewhere), &lab(), &roots(), &runner())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.timings.is_empty());
}

#[test]
fn fewer_than_three_runs_write_nothing_and_say_so() {
    let two = {
        let mut value: serde_json::Value =
            serde_json::from_str(LIST).unwrap_or_else(|e| unreachable!("{e}"));
        value["workflow_runs"] = serde_json::Value::Array(
            value["workflow_runs"]
                .as_array()
                .cloned()
                .unwrap_or_default()[..3]
                .to_vec(),
        );
        value.to_string()
    };
    let report = gather(&recorded(&two), &lab(), &roots(), &runner())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.timings.is_empty());
    assert!(report.notes[0].contains("2 ordinary"), "{:?}", report.notes);
    assert!(report.notes[0].contains("no file is written"));
}

#[test]
fn a_root_whose_steps_cannot_be_read_gets_no_entry() {
    let mut fake = recorded(LIST);
    let repo = lab();
    for &(id, _) in LOGS {
        let log = fake.get_text(&job_log_path(&repo, id));
        fake.respond(
            job_log_path(&repo, id),
            &log.replace("##[group]Run", "##[group]Skip"),
        );
    }
    let report = gather(&fake, &repo, &roots(), &runner()).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.timings.is_empty());
    assert_eq!(report.notes.len(), 5);
    assert!(
        report.notes[3].contains("0 of 3 runs"),
        "{:?}",
        report.notes
    );
    assert!(
        report.notes[..3]
            .iter()
            .all(|note| note.contains("different number of step groups")),
        "{:?}",
        report.notes
    );
}

#[test]
fn a_later_finished_line_in_the_test_step_is_not_the_test_build() {
    let mut fake = recorded(LIST);
    let repo = lab();
    for &(id, _) in LOGS {
        let log = fake.get_text(&job_log_path(&repo, id));
        let later = log.replace(
            "Summary [",
            "Finished `test` profile [unoptimized + debuginfo] target(s) in 99.00s\n2026-11-14T16:54:35.0000000Z Summary [",
        );
        fake.respond(job_log_path(&repo, id), &later);
    }
    let report = gather(&fake, &repo, &roots(), &runner()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(report, self::report(LIST));
}

#[test]
fn only_the_newest_runs_are_read() {
    let mut value: serde_json::Value =
        serde_json::from_str(LIST).unwrap_or_else(|e| unreachable!("{e}"));
    let runs = value["workflow_runs"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let newest = runs[1].clone();
    let mut listed = runs.clone();
    listed.extend(std::iter::repeat_n(newest.clone(), 7));
    for id in 1..=5_u64 {
        let mut stale = newest.clone();
        stale["id"] = id.into();
        listed.push(stale);
    }
    value["workflow_runs"] = serde_json::Value::Array(listed);
    let report = gather(&recorded(&value.to_string()), &lab(), &roots(), &runner())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(report.timings.len(), 2);
}
