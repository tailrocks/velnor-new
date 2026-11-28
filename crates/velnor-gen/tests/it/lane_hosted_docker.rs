// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::TestResult;
use crate::feature_lanes::lane_view;
use crate::pipeline::workflow;
use crate::test_group_partitions::{group, rendered_with};

const HELPER: &str = "0.1.118";
const LANE_ID: &str = "rust-lane-local-mode-root";
const FILTER: &str = "test(~local::) or binary(local_host_public)";
const BASE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::) or binary(local_host_public)'\njob = 'parallel'\n";
const HOSTED: &str = "runner = 'ubuntu-24.04'\nhosted = true\nneeds = ['docker']\n";
const EXCLUSIVE: &str = "exclusive = true\n";

fn jobs_of(config: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let repo = lane_view(HELPER, config)?;
    Ok(crate::yaml_tree::document(&workflow(&repo)?)?["jobs"].clone())
}

fn names(job: &Value) -> Vec<String> {
    job["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|step| step["name"].as_str().map(str::to_owned))
        .collect()
}

fn selections(run: &str) -> Vec<String> {
    run.split("-E \"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .map(str::to_owned)
        .collect()
}

fn default_test_runs(jobs: &Value, id: &str) -> Vec<String> {
    jobs[id]["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|step| step["name"] == "Test")
        .filter_map(|step| step["run"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn a_hosted_docker_lane_runs_on_its_runner_after_a_docker_check() -> TestResult {
    let jobs = jobs_of(&format!("{BASE}{HOSTED}"))?;
    let lane = &jobs[LANE_ID];
    assert_eq!(lane["runs-on"], "ubuntu-24.04", "{lane}");
    assert_eq!(jobs["rust-root"]["runs-on"], "ubuntu-latest");
    let steps = names(lane);
    let check = steps
        .iter()
        .position(|name| name == "Check that Docker answers")
        .ok_or("no docker check")?;
    let test = steps
        .iter()
        .position(|name| name == "Test (features: local-mode)")
        .ok_or("no lane test step")?;
    assert!(check < test, "{steps:?}");
    let run = lane["steps"][check]["run"].as_str().ok_or("no run")?;
    assert_eq!(run, "docker info");
    Ok(())
}

#[test]
fn a_parallel_lane_without_a_runner_or_needs_keeps_the_default_runner_and_adds_no_docker_check()
-> TestResult {
    let jobs = jobs_of(BASE)?;
    assert_eq!(jobs[LANE_ID]["runs-on"], jobs["rust-root"]["runs-on"]);
    assert!(
        names(&jobs[LANE_ID])
            .iter()
            .all(|name| name != "Check that Docker answers")
    );
    Ok(())
}

#[test]
fn an_exclusive_lane_takes_its_tests_out_of_every_default_test_step() -> TestResult {
    let jobs = jobs_of(&format!("{BASE}{HOSTED}{EXCLUSIVE}"))?;
    let defaults = default_test_runs(&jobs, "rust-root");
    assert!(!defaults.is_empty());
    let tail = format!("and not ({FILTER})");
    for run in &defaults {
        let selected = selections(run);
        assert_eq!(selected.len(), 1, "{run}");
        assert!(
            selected[0].starts_with("($TEST_FILTER) ") && selected[0].ends_with(&tail),
            "{run}"
        );
    }
    let other = default_test_runs(&jobs, "rust-tools");
    assert!(!other.is_empty());
    for run in &other {
        assert_eq!(selections(run), ["$TEST_FILTER"], "{run}");
    }
    let lane = jobs[LANE_ID]["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|step| step["name"] == "Test (features: local-mode)")
        .and_then(|step| step["run"].as_str())
        .ok_or("no lane test step")?;
    assert_eq!(selections(lane), [format!("($TEST_FILTER) and ({FILTER})")]);
    Ok(())
}

#[test]
fn a_lane_that_only_reruns_leaves_the_default_test_steps_whole() -> TestResult {
    let jobs = jobs_of(&format!("{BASE}{HOSTED}"))?;
    let defaults = default_test_runs(&jobs, "rust-root");
    assert!(!defaults.is_empty());
    for run in &defaults {
        assert_eq!(selections(run), ["$TEST_FILTER"], "{run}");
    }
    Ok(())
}

#[test]
fn an_exclusive_lane_also_leaves_the_archive_runners_of_the_root() -> TestResult {
    let config = format!(
        "{BASE}{HOSTED}{EXCLUSIVE}{}",
        group("").replace("[[test_groups]]", "[[test_groups]]\nroot = '.'")
    );
    let jobs = rendered_with(&config, HELPER)?["jobs"].clone();
    let tail = format!("and not ({FILTER})");
    let mut seen = 0;
    for (id, job) in jobs.as_object().into_iter().flatten() {
        for step in job["steps"].as_array().into_iter().flatten() {
            let Some(run) = step["run"]
                .as_str()
                .filter(|run| run.contains("--extract-to"))
            else {
                continue;
            };
            seen += 1;
            let selected = selections(run);
            assert!(
                selected.len() == 1 && selected[0].ends_with(&tail),
                "{id}: {run}"
            );
        }
    }
    assert!(seen >= 2, "{seen}");
    Ok(())
}

#[test]
fn a_lane_that_cannot_hold_its_keys_is_refused_naming_the_key() -> TestResult {
    let inline = BASE.replace("job = 'parallel'\n", "");
    for (config, key) in [
        (
            format!("{inline}runner = 'ubuntu-24.04'\nhosted = true\n"),
            "test_lane.runner",
        ),
        (format!("{inline}needs = ['docker']\n"), "test_lane.needs"),
        (format!("{inline}hosted = true\n"), "test_lane.hosted"),
        (
            BASE.replace(
                "filter = 'test(~local::) or binary(local_host_public)'\n",
                "",
            )
            .replace("packages = ['package-0']\n", "")
                + EXCLUSIVE,
            "test_lane.exclusive",
        ),
        (format!("{BASE}needs = ['cargo']\n"), "test_lane.needs"),
        (
            format!("{BASE}needs = ['docker', 'docker']\n"),
            "test_lane.needs",
        ),
        (format!("{BASE}hosted = true\n"), "test_lane.hosted"),
        (format!("{BASE}runner = 'ubuntu-24.04'\n"), "ubuntu-24.04"),
    ] {
        let repo = lane_view(HELPER, &config)?;
        let error = workflow(&repo)
            .err()
            .ok_or_else(|| format!("the configuration was accepted: {config}"))?
            .to_string();
        assert!(error.contains(key), "{key}: {error}");
    }
    Ok(())
}
