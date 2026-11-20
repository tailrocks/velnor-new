// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_ci::collect::collect;
use velnor_ci::github::{job_log_path, jobs_path};
use velnor_ci::logscan::split_steps;
use velnor_ci::timings::gather;
use velnor_model::RepoPath;

use crate::support::{CLI_ID, FakeGitHub, RUN_ID, repo};
use crate::timings::{LIST, lab, recorded, roots, runner};

/// The group markers of a real generated Rust CI job: its `Install velnor-ci`
/// step runs the `setup-velnor` composite action, whose inner `Run set -euo
/// pipefail` group sits between `##[start-action` and `##[end-action`.
const REAL_MARKERS: &str = include_str!("data/timings/composite-job-markers.log");
const REAL_JOBS: &str = include_str!("data/timings/composite-job.json");
const RUNS: [u64; 3] = [43_636_016_430, 43_628_937_052, 43_618_131_619];

const COMPOSITE_LOG: &str = "\
2026-11-14T20:10:04.0000001Z ##[group]Run owner/repo/actions/setup-velnor@0123abc
2026-11-14T20:10:04.0000002Z ##[endgroup]
2026-11-14T20:10:04.0000003Z ##[start-action display=Install velnor-ci;id=__owner_repo.__run]
2026-11-14T20:10:04.0000004Z ##[group]Run set -euo pipefail
2026-11-14T20:10:04.0000005Z ##[endgroup]
2026-11-14T20:10:04.0000006Z ##[end-action id=__owner_repo.__run;outcome=success;conclusion=success;duration_ms=5]
";

/// The recorded runs, each generated job given one extra composite step the way
/// a generated workflow has it (the jobs API lists the step, the log nests an
/// extra `Run` group inside it).
fn composite_runs() -> FakeGitHub {
    let mut fake = recorded(LIST);
    for id in RUNS {
        let path = jobs_path(&lab(), id, 1, 1);
        let mut jobs: Value =
            serde_json::from_str(&fake.get_text(&path)).unwrap_or_else(|e| unreachable!("{e}"));
        for job in jobs["jobs"].as_array_mut().into_iter().flatten() {
            let steps = job["steps"]
                .as_array_mut()
                .unwrap_or_else(|| unreachable!());
            let Some(at) = steps.iter().position(|step| step["name"] == "Check out") else {
                continue;
            };
            let mut composite = steps[at].clone();
            composite["name"] = "Install velnor-ci".into();
            steps.insert(at + 1, composite);
            let log_path = job_log_path(&lab(), job["id"].as_u64().unwrap_or_default());
            let log = fake.get_text(&log_path);
            let marker = log
                .find("##[group]Run jdx/mise-action")
                .unwrap_or_else(|| unreachable!("{log_path}"));
            let line_start = log[..marker].rfind('\n').map_or(0, |at| at + 1);
            let (before, after) = log.split_at(line_start);
            fake.respond(log_path, &format!("{before}{COMPOSITE_LOG}{after}"));
        }
        fake.respond(path, &jobs.to_string());
    }
    fake
}

#[test]
fn a_composite_steps_inner_run_group_does_not_hide_the_timings() {
    let report = gather(&composite_runs(), &lab(), &roots(), &runner())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(report.notes, Vec::<String>::new());
    let root =
        &report.timings[&RepoPath::new("Cargo.toml").unwrap_or_else(|e| unreachable!("{e}"))];
    assert_eq!(root.test_seconds, 20);
    assert_eq!(root.test_build_seconds, 3);
}

#[test]
fn the_real_generated_job_log_splits_into_one_segment_per_top_level_step() {
    let segments = split_steps(REAL_MARKERS);
    // 59 `Run` groups, one of them inside the setup-velnor composite action.
    assert_eq!(segments.len(), 58);
    let job: Value = serde_json::from_str(REAL_JOBS).unwrap_or_else(|e| unreachable!("{e}"));
    let steps = job["jobs"][0]["steps"]
        .as_array()
        .unwrap_or_else(|| unreachable!());
    let user_steps = steps
        .iter()
        .filter(|step| {
            let name = step["name"].as_str().unwrap_or_default();
            step["conclusion"] != "skipped"
                && name != "Set up job"
                && name != "Complete job"
                && !name.starts_with("Post ")
        })
        .count();
    assert_eq!(segments.len(), user_steps);
    assert!(segments[2].starts_with("2026-11-20T09:11:46.9784223Z ##[group]Run tailrocks/"));
    assert!(segments[2].contains("##[group]Run set -euo pipefail"));
    assert!(segments[3].contains("VELNOR_HELPER_INSTALL_LAUNCHER"));
}

#[test]
fn the_real_generated_job_log_pairs_every_step_with_its_own_log() {
    let mut fake = FakeGitHub::recorded();
    let mut jobs: Value = serde_json::from_str(REAL_JOBS).unwrap_or_else(|e| unreachable!("{e}"));
    jobs["jobs"][0]["id"] = CLI_ID.into();
    let mut log = String::new();
    for line in REAL_MARKERS.lines() {
        log.push_str(line);
        log.push('\n');
        if line.contains("##[group]Run") && line.contains("cargo-clippy") {
            log.push_str("2026-11-20T09:12:54.1000000Z Finished `dev` profile [unoptimized] target(s) in 12.50s\n");
        }
    }
    fake.respond(jobs_path(&repo(), RUN_ID, 1, 1), &jobs.to_string());
    fake.respond(job_log_path(&repo(), CLI_ID), &log);
    let run = collect(&fake, &repo(), RUN_ID, false).unwrap_or_else(|e| unreachable!("{e}"));
    let job = &run.jobs[0];
    assert!(!job.step_logs_unmatched);
    let timed: Vec<&str> = job
        .steps
        .iter()
        .filter(|step| !step.log.finished_seconds.is_empty())
        .map(|step| step.name.as_str())
        .collect();
    assert_eq!(timed, ["Lint", "Lint (features: local-mode)"]);
}

#[test]
fn a_log_that_does_not_match_the_steps_is_reported_not_silently_dropped() {
    let mut fake = composite_runs();
    for id in RUNS {
        let path = jobs_path(&lab(), id, 1, 1);
        let jobs: Value =
            serde_json::from_str(&fake.get_text(&path)).unwrap_or_else(|e| unreachable!("{e}"));
        for job in jobs["jobs"].as_array().into_iter().flatten() {
            let log_path = job_log_path(&lab(), job["id"].as_u64().unwrap_or_default());
            let log = fake
                .get_text(&log_path)
                .replace("##[group]Run cargo-fmt", "##[group]Fmt");
            fake.respond(log_path, &log);
        }
    }
    let report = gather(&fake, &lab(), &roots(), &runner()).unwrap_or_else(|e| unreachable!("{e}"));
    let note = report
        .notes
        .iter()
        .find(|note| note.contains("different number of step groups"))
        .unwrap_or_else(|| unreachable!("{:?}", report.notes));
    assert!(note.contains("run 43636016430"), "{note}");
    assert!(note.contains("`rust-root`, `rust-lab`"), "{note}");
    assert_eq!(report.notes.len(), 5, "{:?}", report.notes);
}
