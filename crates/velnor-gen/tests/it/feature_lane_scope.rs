// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_test_support::gh_expr::{Value, evaluate};

use crate::facts::TestResult;
use crate::feature_lanes::{lane_view, steps_named};
use crate::pipeline::workflow;

const LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::)'\n";
const LANE_STEP: &str = "Test (features: local-mode)";
const NOTICE_STEP: &str = "Skip test lane (features: local-mode)";
const NOTICE: &str = "echo 'skipped: no selected packages'";

fn lane_steps(config: &str) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let repo = crate::feature_lanes::lane_view("0.1.93", config)?;
    let jobs = crate::yaml_tree::document(&workflow(&repo)?)?["jobs"].clone();
    let steps: Vec<_> = jobs
        .as_object()
        .into_iter()
        .flatten()
        .flat_map(|(_, job)| job["steps"].as_array().cloned().unwrap_or_default())
        .collect();
    let at = steps
        .iter()
        .position(|step| step["name"] == LANE_STEP)
        .ok_or("no lane step")?;
    Ok(steps[at..].to_vec())
}

fn text<'a>(step: &'a serde_json::Value, key: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    Ok(step[key]
        .as_str()
        .ok_or_else(|| format!("no {key}: {step}"))?)
}

const HELPER: &str = "0.1.93";

fn lane_opens(
    config: &str,
    mode: &'static str,
    packages: &'static str,
) -> Result<Vec<bool>, String> {
    step_opens((HELPER, config), &[LANE_STEP, NOTICE_STEP], mode, packages)
}

#[test]
fn a_full_run_runs_the_lane_and_a_lane_that_selects_nothing_fails() -> TestResult {
    assert_eq!(lane_opens(LANE, "full", "--workspace")?, [true, false]);
    let steps = lane_steps(LANE)?;
    assert!(
        text(&steps[0], "run")?.ends_with("--no-tests=fail"),
        "{}",
        steps[0]
    );
    Ok(())
}

#[test]
fn a_narrowed_run_without_a_lane_package_skips_the_lane_and_says_so() -> TestResult {
    assert_eq!(lane_opens(LANE, "some", "-p elsewhere")?, [false, true]);
    assert_eq!(lane_opens(LANE, "none", "")?, [false, false]);
    let steps = lane_steps(LANE)?;
    assert_eq!(steps[1]["name"], NOTICE_STEP);
    assert_eq!(text(&steps[1], "run")?, NOTICE);
    Ok(())
}

#[test]
fn a_narrowed_run_with_a_lane_package_runs_the_lane_and_fails_without_tests() -> TestResult {
    assert_eq!(lane_opens(LANE, "some", "-p package-0")?, [true, false]);
    let steps = lane_steps(LANE)?;
    let run = text(&steps[0], "run")?;
    assert!(run.ends_with("--no-tests=fail"), "{run}");
    assert!(!run.contains("--no-tests=pass"), "{run}");
    Ok(())
}

#[test]
fn a_lane_without_a_filter_runs_whenever_the_root_is_selected_and_has_no_notice() -> TestResult {
    let config = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\n";
    let steps = lane_steps(config)?;
    assert!(steps.iter().all(|step| step["name"] != NOTICE_STEP));
    assert_eq!(
        step_opens((HELPER, config), &[LANE_STEP], "some", "-p elsewhere")?,
        [true]
    );
    Ok(())
}

#[test]
fn lane_packages_must_be_given_with_a_filter_and_be_packages_of_the_root() -> TestResult {
    for (config, mention) in [
        (
            LANE.replace("packages = ['package-0']\n", ""),
            "test_lane.packages",
        ),
        (LANE.replace("['package-0']", "[]"), "test_lane.packages"),
        (
            LANE.replace("['package-0']", "['package-0', 'package-0']"),
            "test_lane.packages",
        ),
        (
            LANE.replace("['package-0']", "['a b']"),
            "test_lane.packages",
        ),
        (LANE.replace("'package-0'", "'elsewhere'"), "elsewhere"),
        (LANE.replace("'package-0'", "'package-1'"), "package-1"),
    ] {
        let repo = crate::feature_lanes::lane_view("0.1.93", &config)?;
        let error = workflow(&repo)
            .err()
            .ok_or_else(|| format!("the configuration was accepted: {config}"))?;
        assert!(error.to_string().contains(mention), "{config}: {error}");
    }
    Ok(())
}

const LINT_DOC_LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::)'\nlint = true\ndoc = true\n";
const STEPS: [&str; 2] = [
    "Lint (features: local-mode)",
    "Documentation (features: local-mode)",
];

fn plan(mode: &'static str, packages: &'static str) -> impl Fn(&str) -> Result<Value, String> {
    move |name: &str| {
        let text = |value: &str| Ok(Value::Str(value.to_owned()));
        match name {
            "github.event_name" => text("pull_request"),
            "github.ref" => text("refs/pull/7/merge"),
            "github.event.pull_request.state" => text("open"),
            "needs.rust-plan.outputs.skip_push" => text("false"),
            "steps.replan.outcome" => text("skipped"),
            _ if name.ends_with("_mode") => text(mode),
            _ if name.ends_with("_docs") => text("run"),
            _ if name.ends_with("_package_args") => text(packages),
            _ if name.ends_with(".outcome") || name.ends_with(".result") => text("success"),
            _ => Ok(Value::Null),
        }
    }
}

fn step_opens(
    (helper, config): (&str, &str),
    names: &[&str],
    mode: &'static str,
    packages: &'static str,
) -> Result<Vec<bool>, String> {
    let repo = lane_view(helper, config).map_err(|error| error.to_string())?;
    let document = workflow(&repo).map_err(|error| error.to_string())?;
    let jobs =
        crate::yaml_tree::document(&document).map_err(|error| error.to_string())?["jobs"].clone();
    let truthy = |condition: &serde_json::Value| -> Result<bool, String> {
        match condition.as_str() {
            Some(text) => Ok(evaluate(text, &plan(mode, packages))?.truthy()),
            None => Ok(true),
        }
    };
    names
        .iter()
        .map(|name| {
            let found = steps_named(&jobs, name);
            let (job, _, step) = found.first().ok_or(format!("no step {name}"))?;
            Ok(truthy(&jobs[job]["if"])? && truthy(&step["if"])?)
        })
        .collect()
}

fn opens(mode: &'static str, packages: &'static str) -> Result<Vec<bool>, String> {
    step_opens(("0.1.108", LINT_DOC_LANE), &STEPS, mode, packages)
}

#[test]
fn a_change_to_a_dependent_only_opens_the_lane_lint_and_doc_steps() -> TestResult {
    assert_eq!(opens("some", "-p dependent")?, [true, true]);
    Ok(())
}

#[test]
fn a_plan_that_selects_no_member_skips_the_lane_lint_and_doc_steps() -> TestResult {
    assert_eq!(opens("none", "")?, [false, false]);
    Ok(())
}
