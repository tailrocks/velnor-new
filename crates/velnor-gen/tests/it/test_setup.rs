// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const FIXTURE: &str = "[[test_setup]]\nroot = '.'\nname = 'Prepare the fixture'\nrun = 'bash scripts/prepare-fixture.sh'\n";

fn jobs_of(config: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let repo = view(&["Cargo.toml", "tools/Cargo.toml"], Some(config.as_bytes()))?;
    Ok(crate::yaml_tree::document(&workflow(&repo)?)?["jobs"].clone())
}

fn setups(jobs: &serde_json::Value) -> Vec<(String, usize, Option<usize>, serde_json::Value)> {
    let mut found = Vec::new();
    for (id, job) in jobs.as_object().into_iter().flatten() {
        let steps = job["steps"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let tests = steps.iter().position(|step| step["id"] == "test");
        for (index, step) in steps.iter().enumerate() {
            if step["name"] == "Prepare the fixture" {
                found.push((id.clone(), index, tests, step.clone()));
            }
        }
    }
    found
}

#[test]
fn a_root_with_test_setup_runs_it_before_its_tests_and_other_roots_do_not() -> TestResult {
    let found = setups(&jobs_of(FIXTURE)?);
    assert_eq!(found.len(), 1, "{found:?}");
    let (id, setup, tests, step) = &found[0];
    assert_eq!(id, "rust-root");
    assert!(
        tests.is_some_and(|tests| setup < &tests),
        "setup {setup} must precede the tests {tests:?}"
    );
    assert_eq!(step["run"], "bash scripts/prepare-fixture.sh");
    assert!(step["working-directory"].is_null(), "{step}");
    assert!(step["if"].as_str().is_some(), "{step}");
    Ok(())
}

#[test]
fn test_setup_of_a_nested_root_runs_in_that_root() -> TestResult {
    let config = FIXTURE.replace("root = '.'", "root = 'tools'");
    let found = setups(&jobs_of(&config)?);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "rust-tools");
    assert_eq!(found[0].3["working-directory"], "tools");
    Ok(())
}

#[test]
fn test_setup_that_names_no_cargo_root_or_is_not_one_command_is_rejected() -> TestResult {
    for (config, mention) in [
        (FIXTURE.replace("root = '.'", "root = 'missing'"), "missing"),
        (
            FIXTURE.replace("bash scripts", "echo ${{ github.ref }}; bash scripts"),
            "test_setup.run",
        ),
        (
            FIXTURE.replace("bash scripts", "bash a; bash scripts"),
            "`;`",
        ),
        (
            FIXTURE.replace(
                "'bash scripts/prepare-fixture.sh'",
                "'''\nbash a\nbash b\n'''",
            ),
            "test_setup.run",
        ),
        (
            FIXTURE.replace("Prepare the fixture", ""),
            "test_setup.name",
        ),
        (FIXTURE.replace("root = '.'\n", ""), "root"),
    ] {
        let repo = view(&["Cargo.toml", "tools/Cargo.toml"], Some(config.as_bytes()))?;
        let error = workflow(&repo)
            .err()
            .ok_or("the configuration was accepted")?;
        assert!(error.to_string().contains(mention), "{config}: {error}");
    }
    Ok(())
}

#[test]
fn a_failed_test_setup_step_stops_its_tests_and_is_never_continued_past() -> TestResult {
    let jobs = jobs_of(FIXTURE)?;
    let found = setups(&jobs);
    let (id, _, _, setup) = &found[0];
    assert!(setup["continue-on-error"].is_null(), "{setup}");
    let setup_id = setup["id"]
        .as_str()
        .ok_or_else(|| format!("the setup step has no id: {setup}"))?;
    let steps = jobs[id.as_str()]["steps"]
        .as_array()
        .ok_or("the job has no steps")?;
    let test = steps
        .iter()
        .find(|step| step["id"] == "test")
        .ok_or("the test step is missing")?;
    let condition = test["if"].as_str().unwrap_or_default();
    assert!(
        condition.contains(&format!("steps.{setup_id}.outcome == 'success'")),
        "the test step runs after a failed setup: {test}"
    );
    Ok(())
}

#[test]
fn a_failure_report_still_runs_when_test_setup_fails() -> TestResult {
    let jobs = jobs_of(FIXTURE)?;
    let found = setups(&jobs);
    let (id, _, _, _) = &found[0];
    let steps = jobs[id.as_str()]["steps"]
        .as_array()
        .ok_or("the job has no steps")?;
    let report = steps
        .iter()
        .find(|step| step["name"] == "Report cache telemetry")
        .ok_or("the failure report is missing")?;
    let condition = report["if"].as_str().unwrap_or_default();
    assert!(condition.contains("!cancelled()"), "{report}");
    assert!(
        !condition.contains("outcome"),
        "the report waits for a step outcome: {report}"
    );
    Ok(())
}

#[test]
fn a_failed_build_phase_setup_stops_every_cargo_step_after_it() -> TestResult {
    let config = FIXTURE.replace("run = 'bash", "phase = 'build'\nrun = 'bash");
    let jobs = jobs_of(&config)?;
    let found = setups(&jobs);
    let (id, index, _, setup) = &found[0];
    let setup_id = setup["id"]
        .as_str()
        .ok_or_else(|| format!("the setup step has no id: {setup}"))?;
    let steps = jobs[id.as_str()]["steps"]
        .as_array()
        .ok_or("the job has no steps")?;
    let waits = format!("steps.{setup_id}.outcome == 'success'");
    let after: Vec<_> = steps
        .iter()
        .skip(index + 1)
        .filter(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains("cargo"))
        })
        .collect();
    assert!(
        !after.is_empty(),
        "no cargo step follows the build-phase setup"
    );
    for step in after {
        assert!(
            step["if"]
                .as_str()
                .is_some_and(|text| text.contains(&waits)),
            "a cargo step runs after a failed build-phase setup: {step}"
        );
    }
    Ok(())
}
