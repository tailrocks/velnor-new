// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::feature_lanes::lane_view;
use crate::pipeline::workflow;
use velnor_gen::GenerateError;

const FILTER: &str = "test(~local::) or binary(local_host_public)";
const LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::) or binary(local_host_public)'\n";
const GUARD_RUN: &str = ". \"$VELNOR_EXEC\" run -- velnor-ci lane-guard --features 'local-mode' --filter 'test(~local::) or binary(local_host_public)'";

type Guard = (String, usize, serde_json::Value);

fn jobs_of(config: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let repo = lane_view("0.1.86", config)?;
    Ok(crate::yaml_tree::document(&workflow(&repo)?)?["jobs"].clone())
}

fn steps(jobs: &serde_json::Value, matching: impl Fn(&serde_json::Value) -> bool) -> Vec<Guard> {
    let mut found = Vec::new();
    for (id, job) in jobs.as_object().into_iter().flatten() {
        for (index, step) in job["steps"].as_array().into_iter().flatten().enumerate() {
            if matching(step) {
                found.push((id.clone(), index, step.clone()));
            }
        }
    }
    found
}

fn guards(jobs: &serde_json::Value) -> Vec<Guard> {
    steps(jobs, |step| {
        step["run"]
            .as_str()
            .is_some_and(|run| run.contains("lane-guard"))
    })
}

fn lane_steps(jobs: &serde_json::Value) -> Vec<Guard> {
    steps(jobs, |step| step["name"] == "Test (features: local-mode)")
}

#[test]
fn a_filtered_lane_is_followed_by_exactly_one_guard_step() -> TestResult {
    let jobs = jobs_of(LANE)?;
    let found = guards(&jobs);
    assert_eq!(found.len(), 1, "{found:?}");
    let (id, position, step) = &found[0];
    let lane = lane_steps(&jobs);
    assert_eq!(&lane[0].0, id);
    assert!(
        lane[0].1 < *position && *position <= lane[0].1 + 2,
        "{lane:?}"
    );
    assert_eq!(step["name"], "Test lane guard (features: local-mode)");
    assert_eq!(step["run"], GUARD_RUN);
    let run = step["run"].as_str().ok_or("no run")?;
    for forbidden in ["\n", " ; ", "|", "&", "${{", "inputs build"] {
        assert!(!run.contains(forbidden), "{run}");
    }
    assert!(step["env"]["TEST_FILTER"].is_null(), "{step}");
    Ok(())
}

#[test]
fn the_guard_runs_only_when_the_lane_step_runs() -> TestResult {
    let jobs = jobs_of(LANE)?;
    let (guard, lane) = (&guards(&jobs)[0].2["if"], &lane_steps(&jobs)[0].2["if"]);
    assert_eq!(guard, lane);
    Ok(())
}

#[test]
fn an_unfiltered_lane_has_no_guard() -> TestResult {
    let config = LANE
        .replace(&format!("filter = '{FILTER}'\n"), "")
        .replace("packages = ['package-0']\n", "");
    let jobs = jobs_of(&config)?;
    assert_eq!(guards(&jobs).len(), 0);
    assert_eq!(lane_steps(&jobs).len(), 1);
    Ok(())
}

#[test]
fn a_guard_on_a_non_root_lane_runs_in_that_roots_directory() -> TestResult {
    let config = "[[test_lane]]\nroot = 'tools'\nfeatures = ['a', 'b']\npackages = ['package-1']\nfilter = 'test(~x)'\n";
    let found = guards(&jobs_of(config)?);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].2["working-directory"], "tools");
    assert_eq!(
        found[0].2["run"],
        ". \"$VELNOR_EXEC\" run -- velnor-ci lane-guard --features 'a,b' --filter 'test(~x)'"
    );
    Ok(())
}

#[test]
fn a_filtered_lane_needs_helper_0_1_86() -> TestResult {
    let off = format!("full_run = 'off'\n{LANE}");
    for helper in ["0.1.81", "0.1.85"] {
        let error = workflow(&lane_view(helper, &off)?)
            .err()
            .ok_or("an old helper was accepted")?;
        let text = error.to_string();
        assert!(
            text.contains(&format!("velnor-ci {helper} cannot run lane-guard"))
                && text.contains("pin-helper 0.1.86"),
            "{text}"
        );
        assert!(
            error
                .downcast_ref::<GenerateError>()
                .is_some_and(|e| matches!(e, GenerateError::LaneGuardHelperTooOld(_))),
            "{text}"
        );
    }
    assert!(workflow(&lane_view("0.1.86", &off)?).is_ok());
    Ok(())
}
