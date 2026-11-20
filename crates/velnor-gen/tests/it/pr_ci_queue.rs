// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::TestResult;
use crate::pr_ci::{LABEL_CONFIG, render};

const STEP: &str = "velnor-ci merge-queue-check";

fn jobs(version: &str, config: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(crate::yaml_tree::document(&render(version, config)?)?["jobs"].clone())
}

fn step<'a>(job: &'a Value, name: &str) -> Option<&'a Value> {
    job["steps"]
        .as_array()?
        .iter()
        .find(|step| step["name"] == name)
}

fn names(job: &Value) -> Vec<&str> {
    job["steps"]
        .as_array()
        .map_or_default(|steps| steps.iter().filter_map(|s| s["name"].as_str()).collect())
}

fn calls_it(jobs: &Value) -> bool {
    jobs.as_object()
        .is_some_and(|all| all.values().any(|job| step(job, STEP).is_some()))
}

#[test]
fn label_mode_checks_the_merge_queue_in_the_plan_job_right_after_the_plan() -> TestResult {
    let all = jobs("0.1.100", LABEL_CONFIG)?;
    let plan = &all["rust-plan"];
    let names = names(plan);
    let at = names
        .iter()
        .position(|name| *name == "velnor-ci plan")
        .ok_or("plan")?;
    assert_eq!(names.get(at + 1), Some(&STEP));
    let check = step(plan, STEP).ok_or("check")?;
    assert_eq!(check["continue-on-error"], true);
    assert_eq!(check["env"]["GITHUB_TOKEN"], "${{ github.token }}");
    let when = check["if"].as_str().unwrap_or_default();
    assert!(
        when.contains("github.event_name == 'pull_request'"),
        "{when}"
    );
    assert!(
        check["run"]
            .as_str()
            .is_some_and(|run| run.ends_with("merge-queue-check"))
    );
    assert!(step(&all["required"], STEP).is_none());
    Ok(())
}

#[test]
fn the_check_adds_no_permission_to_the_plan_job() -> TestResult {
    let with = jobs("0.1.100", LABEL_CONFIG)?;
    let without = jobs("0.1.98", LABEL_CONFIG)?;
    assert_eq!(
        with["rust-plan"]["permissions"],
        without["rust-plan"]["permissions"]
    );
    Ok(())
}

#[test]
fn a_helper_before_the_command_gets_no_check_and_the_workflow_still_renders() -> TestResult {
    for version in ["0.1.88", "0.1.98", "0.1.99"] {
        assert!(!calls_it(&jobs(version, LABEL_CONFIG)?), "{version}");
    }
    Ok(())
}

#[test]
fn always_mode_never_checks_for_a_merge_queue() -> TestResult {
    let always = "shape = \"lint-test\"\npr_ci = \"always\"\n";
    assert!(!calls_it(&jobs("0.1.100", always)?));
    assert!(!calls_it(&jobs("0.1.100", "shape = \"lint-test\"\n")?));
    Ok(())
}

#[test]
fn the_single_job_layout_checks_in_the_job_after_its_plan() -> TestResult {
    let all = jobs("0.1.100", "pr_ci = \"label\"\n")?;
    let job = &all["rust"];
    let names = names(job);
    let at = names
        .iter()
        .position(|name| *name == "velnor-ci plan")
        .ok_or("plan")?;
    assert_eq!(names.get(at + 1), Some(&STEP));
    Ok(())
}
