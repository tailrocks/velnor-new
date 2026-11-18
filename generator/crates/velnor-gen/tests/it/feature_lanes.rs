// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\nfilter = 'test(~local::) or binary(local_host_public)'\n";
const LANE_STEP: &str = "Test (features: local-mode)";

fn jobs_of(config: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let repo = view(&["Cargo.toml", "tools/Cargo.toml"], Some(config.as_bytes()))?;
    Ok(crate::yaml_tree::document(&workflow(&repo)?)?["jobs"].clone())
}

fn steps_named(jobs: &serde_json::Value, name: &str) -> Vec<(String, usize, serde_json::Value)> {
    let mut found = Vec::new();
    for (id, job) in jobs.as_object().into_iter().flatten() {
        for (index, step) in job["steps"].as_array().into_iter().flatten().enumerate() {
            if step["name"] == name {
                found.push((id.clone(), index, step.clone()));
            }
        }
    }
    found
}

#[test]
fn a_feature_lane_runs_its_own_nextest_step_with_the_features_and_the_filter() -> TestResult {
    let jobs = jobs_of(LANE)?;
    let lane = steps_named(&jobs, LANE_STEP);
    assert_eq!(lane.len(), 1, "{lane:?}");
    let (id, position, step) = &lane[0];
    assert_eq!(id, "rust-root");
    let run = step["run"].as_str().ok_or("no run")?;
    assert!(
        run.contains("nextest run --workspace --locked --features 'local-mode'"),
        "{run}"
    );
    assert!(
        run.contains("-E \"($TEST_FILTER) and (test(~local::) or binary(local_host_public))\""),
        "{run}"
    );
    assert!(step["id"].is_null(), "{step}");
    let defaults: Vec<_> = steps_named(&jobs, "Test")
        .into_iter()
        .filter(|(job, ..)| job == "rust-root")
        .collect();
    assert_eq!(defaults.len(), 1, "{defaults:?}");
    let default_run = defaults[0].2["run"].as_str().ok_or("no run")?;
    assert!(!default_run.contains("--features"), "{default_run}");
    assert!(defaults[0].1 < *position, "the default tests run first");
    Ok(())
}

#[test]
fn a_feature_lane_without_a_filter_runs_the_whole_selected_set_with_the_features() -> TestResult {
    let config = LANE.replace(
        "filter = 'test(~local::) or binary(local_host_public)'\n",
        "",
    );
    let lane = steps_named(&jobs_of(&config)?, LANE_STEP);
    let run = lane[0].2["run"].as_str().ok_or("no run")?;
    assert!(run.contains("--features 'local-mode'"), "{run}");
    assert!(run.contains("-E \"$TEST_FILTER\""), "{run}");
    Ok(())
}

#[test]
fn lanes_of_other_roots_and_several_features_render_per_root() -> TestResult {
    let config = "[[test_lane]]\nroot = 'tools'\nfeatures = ['a', 'b']\n";
    let jobs = jobs_of(config)?;
    let lane = steps_named(&jobs, "Test (features: a,b)");
    assert_eq!(lane.len(), 1, "{lane:?}");
    assert_eq!(lane[0].0, "rust-tools");
    assert_eq!(lane[0].2["working-directory"], "tools");
    assert!(
        lane[0].2["run"]
            .as_str()
            .ok_or("no run")?
            .contains("--features 'a,b'")
    );
    Ok(())
}

#[test]
fn in_the_groups_shape_the_feature_lane_runs_where_rust_is_installed() -> TestResult {
    let config = format!("shape = 'groups'\n{LANE}");
    let jobs = jobs_of(&config)?;
    let lane = steps_named(&jobs, LANE_STEP);
    assert_eq!(lane.len(), 1, "{lane:?}");
    assert_eq!(lane[0].0, "rust-test-root");
    Ok(())
}

#[test]
fn a_feature_lane_that_is_not_one_safe_command_is_rejected() -> TestResult {
    for (config, mention) in [
        (LANE.replace("root = '.'\n", ""), "root"),
        (LANE.replace("root = '.'", "root = 'missing'"), "missing"),
        (LANE.replace("['local-mode']", "[]"), "test_lane.features"),
        (LANE.replace("['local-mode']", "['']"), "test_lane.features"),
        (
            LANE.replace("['local-mode']", "['a b']"),
            "test_lane.features",
        ),
        (
            LANE.replace("['local-mode']", "['a,b']"),
            "test_lane.features",
        ),
        (
            LANE.replace("['local-mode']", "['a', 'a']"),
            "test_lane.features",
        ),
        (
            LANE.replace("'test(~local::) or binary(local_host_public)'", "''"),
            "test_lane.filter",
        ),
        (
            LANE.replace("test(~local::) or", "test(~local::) |"),
            "test_lane.filter",
        ),
        (
            LANE.replace("test(~local::) or", "test(~local::) ;"),
            "test_lane.filter",
        ),
        (
            LANE.replace("test(~local::) or", "test(~local::) & binary(x) or"),
            "test_lane.filter",
        ),
        (
            LANE.replace("test(~local::)", "test(~\"x\")"),
            "test_lane.filter",
        ),
        (
            LANE.replace("test(~local::)", "test(~$X)"),
            "test_lane.filter",
        ),
        (
            LANE.replace("test(~local::)", "test(~`x`)"),
            "test_lane.filter",
        ),
        (
            LANE.replace("test(~local::) or", "test(~local::) or if"),
            "test_lane.filter",
        ),
        (
            LANE.replace(
                "filter = 'test(~local::) or binary(local_host_public)'",
                "filter = '''\ntest(~a)\nor test(~b)\n'''",
            ),
            "test_lane.filter",
        ),
        (format!("{LANE}extra = 1\n"), "extra"),
    ] {
        let repo = view(&["Cargo.toml", "tools/Cargo.toml"], Some(config.as_bytes()))?;
        let error = workflow(&repo)
            .err()
            .ok_or_else(|| format!("the configuration was accepted: {config}"))?;
        assert!(error.to_string().contains(mention), "{config}: {error}");
    }
    Ok(())
}
