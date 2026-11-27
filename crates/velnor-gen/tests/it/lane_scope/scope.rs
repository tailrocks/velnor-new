// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::workflow;

const LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::)'\n";
const LANE_STEP: &str = "Test (features: local-mode)";
const NOTICE_STEP: &str = "Skip test lane (features: local-mode)";
const NOTICE: &str = "echo 'skipped: no selected packages'";
const WORKSPACE: &str = "contains(format(' {0} ', (steps.replan.outputs.rust_root_package_args || needs.rust-plan.outputs.rust_root_package_args)), ' --workspace ')";

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

fn package(name: &str) -> String {
    format!(
        "contains(format(' {{0}} ', (steps.replan.outputs.rust_root_package_args || needs.rust-plan.outputs.rust_root_package_args)), ' -p {name} ')"
    )
}

fn text<'a>(step: &'a serde_json::Value, key: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    Ok(step[key]
        .as_str()
        .ok_or_else(|| format!("no {key}: {step}"))?)
}

#[test]
fn a_full_run_runs_the_lane_and_a_lane_that_selects_nothing_fails() -> TestResult {
    let steps = lane_steps(LANE)?;
    let condition = text(&steps[0], "if")?;
    assert!(condition.contains(WORKSPACE), "{condition}");
    assert!(
        text(&steps[0], "run")?.ends_with("--no-tests=fail"),
        "{}",
        steps[0]
    );
    Ok(())
}

#[test]
fn a_narrowed_run_without_a_lane_package_skips_the_lane_and_says_so() -> TestResult {
    let steps = lane_steps(LANE)?;
    let (lane, notice) = (text(&steps[0], "if")?, &steps[1]);
    assert_eq!(notice["name"], NOTICE_STEP);
    assert_eq!(text(notice, "run")?, NOTICE);
    let condition = text(notice, "if")?;
    let wanted = [package("package-0"), WORKSPACE.to_owned()];
    let inside = format!("!({})", wanted.join(" || "));
    assert!(condition.contains(&inside), "{condition}");
    assert!(condition.contains("!= 'none'"), "{condition}");
    assert!(
        lane.contains(&format!("({})", wanted.join(" || "))),
        "{lane}"
    );
    assert!(!lane.contains("!("), "{lane}");
    Ok(())
}

#[test]
fn a_narrowed_run_with_a_lane_package_runs_the_lane_and_fails_without_tests() -> TestResult {
    let steps = lane_steps(LANE)?;
    let condition = text(&steps[0], "if")?;
    assert!(
        condition.contains(&package("package-0")),
        "package-0: {condition}"
    );
    let run = text(&steps[0], "run")?;
    assert!(run.ends_with("--no-tests=fail"), "{run}");
    assert!(!run.contains("--no-tests=pass"), "{run}");
    Ok(())
}

#[test]
fn a_lane_without_a_filter_runs_whenever_the_root_is_selected_and_has_no_notice() -> TestResult {
    let steps = lane_steps("[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\n")?;
    assert!(
        !text(&steps[0], "if")?.contains("package_args"),
        "{}",
        steps[0]
    );
    assert!(steps.iter().all(|step| step["name"] != NOTICE_STEP));
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
