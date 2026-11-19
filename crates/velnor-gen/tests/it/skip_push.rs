// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_gen::expected_with_notes;
use velnor_model::RepoView;

use crate::facts::{TestResult, path};
use crate::pipeline::{view, workflow};

const KNOB: &[u8] = b"[ci]\nskip_push_after_merge_group = true\n";
const SINGLE_SKIP: &str = "steps.merge_group.outputs.skip != 'true'";
const PLANNED_SKIP: &str = "needs.rust-plan.outputs.skip_push != 'true'";

fn repo(
    roots: &[&str],
    config: Option<&[u8]>,
    helper: &str,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(roots, config)?;
    let lock = path(".velnor/rust-ci.lock")?;
    let old = String::from_utf8(repo.files[&lock].clone())?;
    repo.files
        .insert(lock, old.replace("0.1.81", helper).into_bytes());
    Ok(repo)
}

fn jobs(text: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(crate::yaml_tree::document(text)?["jobs"].clone())
}

fn step<'a>(job: &'a Value, name: &str) -> Option<&'a Value> {
    job["steps"]
        .as_array()?
        .iter()
        .find(|step| step["name"] == name)
}

fn condition(job: &Value, name: &str) -> String {
    step(job, name)
        .and_then(|step| step["if"].as_str())
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn knob_off_or_false_leaves_the_workflow_byte_identical() -> TestResult {
    let base = workflow(&view(&["Cargo.toml"], None)?)?;
    let off = workflow(&view(
        &["Cargo.toml"],
        Some(b"[ci]\nskip_push_after_merge_group = false\n"),
    )?)?;
    assert_eq!(base, off);
    assert!(!base.contains("merge-group-passed"), "{base}");
    assert!(!base.contains("skip_push"), "{base}");
    Ok(())
}

#[test]
fn knob_needs_a_helper_that_knows_the_command() -> TestResult {
    for older in ["0.1.81", "0.1.82", "0.1.83", "0.1.84", "0.1.85", "0.1.87"] {
        let old = repo(&["Cargo.toml"], Some(KNOB), older)?;
        let error = expected_with_notes(&old)
            .err()
            .ok_or(format!("{older} accepted"))?
            .to_string();
        assert!(error.contains("pin-helper 0.1.88"), "{older}: {error}");
    }
    workflow(&repo(&["Cargo.toml"], Some(KNOB), "0.1.88")?)?;
    Ok(())
}

#[test]
fn unknown_ci_key_is_refused() -> TestResult {
    let bad = repo(
        &["Cargo.toml"],
        Some(b"[ci]\nskip_everything = true\n"),
        "0.1.88",
    )?;
    assert!(expected_with_notes(&bad).is_err());
    Ok(())
}

#[test]
fn single_job_asks_the_helper_after_the_plan_and_skips_only_checks() -> TestResult {
    let text = workflow(&repo(&["Cargo.toml"], Some(KNOB), "0.1.88")?)?;
    let all = jobs(&text)?;
    let job = &all["rust"];
    let names: Vec<&str> = job["steps"]
        .as_array()
        .ok_or("steps")?
        .iter()
        .filter_map(|step| step["name"].as_str())
        .collect();
    let plan = names
        .iter()
        .position(|n| *n == "velnor-ci plan")
        .ok_or("plan")?;
    assert_eq!(names[plan + 1], "velnor-ci merge-group-passed");
    let ask = step(job, "velnor-ci merge-group-passed").ok_or("ask")?;
    assert_eq!(ask["id"], "merge_group");
    assert_eq!(ask["continue-on-error"], true);
    assert_eq!(ask["env"]["GITHUB_TOKEN"], "${{ github.token }}");
    let when = ask["if"].as_str().unwrap_or_default();
    assert!(when.contains("github.event_name == 'push'"), "{when}");
    assert!(when.contains("default_branch"), "{when}");
    assert_eq!(job["permissions"]["actions"], "read");
    assert_eq!(job["permissions"].as_object().ok_or("p")?.len(), 2);
    assert_eq!(
        job["outputs"]["skip_push"],
        "${{ steps.merge_group.outputs.skip }}"
    );
    for skipped in ["Format", "Lint", "Test"] {
        assert!(condition(job, skipped).contains(SINGLE_SKIP), "{skipped}");
    }
    for kept in [
        "Build the tests",
        "Save the target cache",
        "Save the tools cache",
        "Check out",
    ] {
        assert!(!condition(job, kept).contains("merge_group"), "{kept}");
    }
    assert!(!text.contains("rust-plan"), "{text}");
    assert!(!waived_on_push(&all["required"]), "{text}");
    Ok(())
}

#[test]
fn planned_lanes_read_the_plan_job_flag_and_cache_jobs_keep_running() -> TestResult {
    let text = workflow(&repo(
        &["Cargo.toml", "lab/Cargo.toml"],
        Some(KNOB),
        "0.1.88",
    )?)?;
    let all = jobs(&text)?;
    let plan = &all["rust-plan"];
    assert!(step(plan, "velnor-ci merge-group-passed").is_some());
    assert_eq!(plan["permissions"]["actions"], "read");
    assert_eq!(
        plan["outputs"]["skip_push"],
        "${{ steps.merge_group.outputs.skip }}"
    );
    let mut skipped = 0;
    for (id, job) in all.as_object().ok_or("jobs")? {
        for step in job["steps"].as_array().into_iter().flatten() {
            let name = step["name"].as_str().unwrap_or_default();
            let when = step["if"].as_str().unwrap_or_default();
            let check = matches!(name, "Format" | "Lint" | "Test" | "Documentation");
            let cache = name.starts_with("Save ") || name == "Build the tests";
            if check {
                assert!(when.contains(PLANNED_SKIP), "{id} {name}");
                skipped += 1;
            }
            if cache {
                assert!(!when.contains("skip_push"), "{id} {name}");
            }
        }
    }
    assert!(skipped >= 4, "{text}");
    let required = &all["required"];
    assert!(step(required, "velnor-ci gate").is_some(), "{required}");
    assert!(!waived_on_push(required), "{required}");
    Ok(())
}

/// True when the job or any of its steps has a condition that mentions the
/// queue-run skip. The gate must never carry one, or a skipped push would
/// pass without running the gate.
fn waived_on_push(job: &Value) -> bool {
    let mentions = |text: Option<&str>| {
        text.is_some_and(|text| text.contains("merge_group") || text.contains("skip_push"))
    };
    mentions(job["if"].as_str())
        || job["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|step| mentions(step["if"].as_str()))
}

#[test]
fn the_plan_job_holds_actions_read_only_and_nothing_gains_actions_write() -> TestResult {
    let text = workflow(&repo(
        &["Cargo.toml", "lab/Cargo.toml"],
        Some(KNOB),
        "0.1.88",
    )?)?;
    let base = workflow(&repo(&["Cargo.toml", "lab/Cargo.toml"], None, "0.1.88")?)?;
    let (all, before) = (jobs(&text)?, jobs(&base)?);
    assert_eq!(
        all["rust-plan"]["permissions"]
            .as_object()
            .ok_or("p")?
            .len(),
        2
    );
    for (id, job) in all.as_object().ok_or("jobs")? {
        assert_eq!(
            job["permissions"]["actions"] == "write",
            before[id]["permissions"]["actions"] == "write",
            "{id}"
        );
    }
    Ok(())
}
