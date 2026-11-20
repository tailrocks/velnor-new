// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde_json::Value;
use velnor_gen::GenerateError;
use velnor_model::{JobId, VerdictSpec};
use velnor_rust::{Need, decide};

use crate::facts::TestResult;
use crate::feature_lanes::lane_view;
use crate::pipeline::{pin_helper, view, workflow};

const HELPER: &str = "0.1.107";
const LANE_ID: &str = "rust-lane-local-mode-root";
const FILTERED: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::) or binary(local_host_public)'\nlint = true\ndoc = true\n";
const UNFILTERED: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\n";
const PARALLEL: &str = "job = 'parallel'\n";

type Jobs = Value;

fn with_job(lane: &str) -> String {
    format!("{lane}{PARALLEL}")
}

fn jobs_of(config: &str) -> Result<Jobs, Box<dyn std::error::Error>> {
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

fn position(names: &[String], name: &str) -> Result<usize, Box<dyn std::error::Error>> {
    names
        .iter()
        .position(|candidate| candidate == name)
        .ok_or_else(|| format!("no step {name} in {names:?}").into())
}

#[test]
fn a_parallel_lane_runs_its_steps_in_its_own_job_after_the_plan() -> TestResult {
    let jobs = jobs_of(&with_job(FILTERED))?;
    let lane = &jobs[LANE_ID];
    assert_eq!(lane["needs"], serde_json::json!(["rust-plan"]), "{lane}");
    let steps = names(lane);
    let test = position(&steps, "Test (features: local-mode)")?;
    let guard = position(&steps, "Test lane guard (features: local-mode)")?;
    let lint = position(&steps, "Lint (features: local-mode)")?;
    let doc = position(&steps, "Documentation (features: local-mode)")?;
    assert!(test < guard && guard < lint && lint < doc, "{steps:?}");
    let condition = lane["if"].as_str().ok_or("no job condition")?;
    assert!(
        condition.contains("needs.rust-plan.outputs.rust_root_mode != 'none'"),
        "{condition}"
    );
    assert_eq!(lane["permissions"]["contents"], "read");
    assert_eq!(lane["permissions"]["actions"], "read");
    assert!(lane["permissions"]["actions"] != "write");
    for default in ["rust-root", "rust-tools"] {
        let others = names(&jobs[default]);
        assert!(
            others.iter().all(|name| !name.contains("features:")),
            "{default}: {others:?}"
        );
    }
    Ok(())
}

#[test]
fn the_required_job_waits_for_the_lane_job_under_its_one_check_name() -> TestResult {
    let jobs = jobs_of(&with_job(FILTERED))?;
    let needs = jobs["required"]["needs"].as_array().ok_or("no needs")?;
    assert!(needs.iter().any(|need| need == LANE_ID), "{needs:?}");
    assert_eq!(jobs["required"]["name"], "Required");
    assert!(jobs[LANE_ID]["name"].is_null(), "{}", jobs[LANE_ID]);
    Ok(())
}

#[test]
fn the_lane_job_restores_and_saves_only_its_own_target_cache_on_the_default_branch() -> TestResult {
    let jobs = jobs_of(&with_job(FILTERED))?;
    let steps = jobs[LANE_ID]["steps"].as_array().ok_or("no steps")?;
    let saves: Vec<&Value> = steps
        .iter()
        .filter(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("actions/cache/save@"))
        })
        .filter(|step| step["with"]["path"] == "target")
        .collect();
    assert_eq!(saves.len(), 1, "{saves:?}");
    let save = saves[0];
    assert!(
        save["with"]["key"]
            .as_str()
            .ok_or("no key")?
            .starts_with(&format!("velnor-tgt-{LANE_ID}.")),
        "{save}"
    );
    let condition = save["if"].as_str().ok_or("no condition")?;
    assert!(
        condition.contains("github.event.repository.default_branch")
            && condition.contains("github.event_name == 'push'"),
        "{condition}"
    );
    let restore = steps
        .iter()
        .find(|step| step["id"] == "target_root")
        .ok_or("no target restore")?;
    assert!(
        restore["with"]["key"]
            .as_str()
            .ok_or("no key")?
            .starts_with(&format!("velnor-tgt-{LANE_ID}.")),
        "{restore}"
    );
    for other in ["rust-root", "rust-tools"] {
        for step in jobs[other]["steps"].as_array().into_iter().flatten() {
            let key = step["with"]["key"].as_str().unwrap_or_default();
            assert!(!key.contains(LANE_ID), "{other}: {step}");
        }
    }
    Ok(())
}

#[test]
fn a_lane_without_the_job_key_stays_inline_and_renders_as_before() -> TestResult {
    let plain = workflow(&lane_view(HELPER, FILTERED)?)?;
    let explicit = workflow(&lane_view(HELPER, &format!("{FILTERED}job = 'inline'\n"))?)?;
    assert_eq!(plain, explicit);
    assert!(!plain.contains("rust-lane-"), "{plain}");
    let jobs = jobs_of(FILTERED)?;
    assert!(
        names(&jobs["rust-root"])
            .iter()
            .any(|name| name == "Test (features: local-mode)")
    );
    assert!(jobs[LANE_ID].is_null());
    Ok(())
}

#[test]
fn a_single_root_with_a_parallel_lane_gets_a_plan_job_and_a_lane_job() -> TestResult {
    let mut repo = view(&["Cargo.toml"], Some(with_job(UNFILTERED).as_bytes()))?;
    pin_helper(&mut repo, HELPER)?;
    let jobs = crate::yaml_tree::document(&workflow(&repo)?)?["jobs"].clone();
    for id in ["rust-plan", "rust", "rust-lane-local-mode"] {
        assert!(jobs[id].is_object(), "no job {id}");
    }
    assert_eq!(jobs["rust-lane-local-mode"]["needs"][0], "rust-plan");
    assert!(
        !names(&jobs["rust"])
            .iter()
            .any(|name| name.contains("features:"))
    );
    let mut inline = view(&["Cargo.toml"], Some(UNFILTERED.as_bytes()))?;
    pin_helper(&mut inline, HELPER)?;
    let jobs = crate::yaml_tree::document(&workflow(&inline)?)?["jobs"].clone();
    assert!(jobs["rust-plan"].is_null());
    Ok(())
}

#[test]
fn the_lane_job_reports_its_test_counts_and_reads_zero_when_no_package_is_selected() -> TestResult {
    let filtered = jobs_of(&with_job(FILTERED))?;
    let outputs = &filtered[LANE_ID]["outputs"];
    assert_eq!(
        outputs["tests_run"],
        "${{ steps.test.outputs.tests_run || '0' }}"
    );
    assert_eq!(
        outputs["tests_listed"],
        "${{ steps.test.outputs.tests_listed || '0' }}"
    );
    assert_eq!(
        outputs["tests_by_package"],
        "${{ steps.test.outputs.tests_by_package }}"
    );
    let test = filtered[LANE_ID]["steps"]
        .as_array()
        .ok_or("no steps")?
        .iter()
        .find(|step| step["name"] == "Test (features: local-mode)")
        .ok_or("no lane step")?;
    assert_eq!(test["id"], "test");
    let unfiltered = jobs_of(&with_job(UNFILTERED))?;
    assert_eq!(
        unfiltered[LANE_ID]["outputs"]["tests_run"],
        "${{ steps.test.outputs.tests_run }}"
    );
    Ok(())
}

fn need(result: &str, outputs: &[(&str, &str)]) -> Need {
    Need {
        result: Some(result.to_owned()),
        outputs: outputs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn counted(packages: &str, run: u64, listed: u64) -> Need {
    need(
        "success",
        &[
            ("tests_by_package", packages),
            ("tests_run", &run.to_string()),
            ("tests_listed", &listed.to_string()),
        ],
    )
}

fn gate_spec() -> Result<VerdictSpec, Box<dyn std::error::Error>> {
    let jobs = jobs_of(&with_job(FILTERED))?;
    let steps = jobs["required"]["steps"].as_array().ok_or("no steps")?;
    let text = steps
        .iter()
        .find_map(|step| step["env"]["GATE_SPEC"].as_str())
        .ok_or("no gate spec")?;
    Ok(VerdictSpec::from_text(text)?)
}

fn verdict(default: Need, lane: Need) -> Result<velnor_rust::Verdict, Box<dyn std::error::Error>> {
    let spec = gate_spec()?;
    let id = |text: &str| JobId::new(text).map_err(|e| e.to_string());
    let needs = BTreeMap::from([
        (
            id("rust-plan")?,
            need(
                "success",
                &[
                    ("rust_root_mode", "some"),
                    ("rust_root_tests", "run"),
                    ("rust_tools_mode", "none"),
                    ("rust_tools_tests", "skip"),
                ],
            ),
        ),
        (id("rust-root")?, default),
        (id("rust-tools")?, need("skipped", &[])),
        (id(LANE_ID)?, lane),
    ]);
    Ok(decide(&spec, &needs, Some("pull_request")))
}

#[test]
fn the_gate_names_the_lane_job_as_a_job_whose_test_counts_it_reads() -> TestResult {
    let spec = gate_spec()?;
    let lane = spec
        .jobs
        .iter()
        .find(|job| job.id.as_str() == LANE_ID)
        .ok_or("the gate does not know the lane job")?;
    assert_eq!(
        lane.tests
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["root"]
    );
    Ok(())
}

#[test]
fn the_gate_fails_when_the_lane_job_lists_tests_of_a_package_and_runs_none() -> TestResult {
    let default = counted("package-0=2/2", 2, 2);
    let broken = verdict(default.clone(), counted("package-1=0/4", 0, 4))?;
    assert!(!broken.passed, "{}", broken.table());
    assert!(
        broken
            .table()
            .contains("package package-1 of root listed 4 tests and ran none"),
        "{}",
        broken.table()
    );
    let healthy = verdict(default, counted("package-1=3/4", 3, 4))?;
    assert!(healthy.passed, "{}", healthy.table());
    Ok(())
}

#[test]
fn the_gate_fails_when_the_lane_job_reports_no_counts_at_all() -> TestResult {
    let default = counted("package-0=2/2", 2, 2);
    let silent = verdict(default.clone(), need("success", &[]))?;
    assert!(!silent.passed, "{}", silent.table());
    let skipped = verdict(default, counted("", 0, 0))?;
    assert!(skipped.passed, "{}", skipped.table());
    Ok(())
}

#[test]
fn a_parallel_lane_needs_a_helper_that_reads_the_job_key() -> TestResult {
    for helper in ["0.1.93", "0.1.100", "0.1.106"] {
        let error = workflow(&lane_view(helper, &with_job(FILTERED))?)
            .err()
            .ok_or("an old helper was accepted")?;
        assert!(
            error
                .downcast_ref::<GenerateError>()
                .is_some_and(|e| matches!(e, GenerateError::ConfigKeyHelperTooOld { key: "test_lane.job", .. })),
            "{error}"
        );
        assert!(
            workflow(&lane_view(helper, FILTERED)?).is_ok(),
            "an inline lane needs no new helper at {helper}"
        );
    }
    Ok(())
}

#[test]
fn an_explicit_inline_job_key_needs_the_helper_that_reads_it() -> TestResult {
    // The pinned 0.1.106 rejects the `job` key whatever its value (deny_unknown_fields).
    let inline = format!("{FILTERED}job = 'inline'\n");
    let error = workflow(&lane_view("0.1.106", &inline)?)
        .err()
        .ok_or("a 0.1.106 pin accepted an explicit job = 'inline' key")?;
    assert!(
        error
            .downcast_ref::<GenerateError>()
            .is_some_and(|e| matches!(e, GenerateError::ConfigKeyHelperTooOld { key: "test_lane.job", .. })),
        "{error}"
    );
    assert!(
        workflow(&lane_view(HELPER, &inline)?).is_ok(),
        "the new helper must accept an explicit inline job key"
    );
    Ok(())
}

#[test]
fn an_unknown_job_value_is_refused() -> TestResult {
    let error = workflow(&lane_view(
        HELPER,
        &format!("{FILTERED}job = 'sideways'\n"),
    )?)
    .err()
    .ok_or("an unknown job value was accepted")?;
    assert!(error.to_string().contains("test_lane.job"), "{error}");
    Ok(())
}

#[test]
fn two_parallel_lanes_with_the_same_features_get_distinct_jobs() -> TestResult {
    let twice = format!(
        "{}{}",
        with_job(
            "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~a::)'\n"
        ),
        with_job(
            "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~b::)'\n"
        )
    );
    let jobs = jobs_of(&twice)?;
    assert!(jobs[LANE_ID].is_object());
    assert!(jobs["rust-lane-local-mode-2-root"].is_object());
    Ok(())
}
