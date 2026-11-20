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

const LINT_LANE: &str =
    "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\nlint = true\ndoc = true\n";

#[test]
fn a_clippy_or_doc_step_after_the_tests_does_not_wait_for_the_test_setup() -> TestResult {
    let jobs = crate::feature_lanes::jobs_of(&format!("{FIXTURE}{LINT_LANE}"))?;
    let mut checked = 0;
    for (id, job) in jobs.as_object().into_iter().flatten() {
        for step in job["steps"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let run = step["run"].as_str().unwrap_or_default();
            if !(run.contains("cargo clippy") || run.contains("cargo doc")) {
                continue;
            }
            checked += 1;
            let condition = step["if"].as_str().unwrap_or_default();
            assert!(
                !condition.contains("test_setup_"),
                "{id}: a quick check waits for the test setup: {step}"
            );
        }
    }
    assert!(checked > 0, "no clippy or doc step was rendered");
    Ok(())
}

#[test]
fn a_root_setup_in_a_workspace_group_does_not_wait_for_another_roots_setup() -> TestResult {
    let config = "[[workspace_groups]]\nname = 'tools'\nroots = ['tools/a', 'tools/b']\n\
        [[test_setup]]\nroot = 'tools/a'\nname = 'Prepare a'\nrun = 'bash scripts/a.sh'\n\
        [[test_setup]]\nroot = 'tools/b'\nname = 'Prepare b'\nrun = 'bash scripts/b.sh'\n";
    let mut repo = view(
        &["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"],
        Some(config.as_bytes()),
    )?;
    crate::pipeline::pin_helper(&mut repo, "0.1.96")?;
    let jobs = crate::yaml_tree::document(&workflow(&repo)?)?["jobs"].clone();
    let steps_of = |name: &str| -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
        Ok(jobs
            .as_object()
            .into_iter()
            .flatten()
            .flat_map(|(_, job)| job["steps"].as_array().cloned().unwrap_or_default())
            .filter(|step| step["name"] == name)
            .collect())
    };
    let a = steps_of("Prepare a")?;
    let b = steps_of("Prepare b")?;
    assert_eq!((a.len(), b.len()), (1, 1), "{jobs}");
    let a_id = a[0]["id"].as_str().ok_or("the setup of a has no id")?;
    let waits_for_a = format!("steps.{a_id}.outcome");
    let condition = b[0]["if"].as_str().unwrap_or_default();
    assert!(
        !condition.contains(&waits_for_a),
        "the setup of b waits for the setup of a: {}",
        b[0]
    );
    Ok(())
}

#[test]
fn a_nested_root_test_in_a_workspace_group_waits_for_its_own_setup() -> TestResult {
    let config = "[[workspace_groups]]\nname = 'tools'\nroots = ['tools/a', 'tools/b']\n\
        [[test_setup]]\nroot = 'tools/a'\nname = 'Prepare a'\nrun = 'bash scripts/a.sh'\n";
    let mut repo = view(
        &["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"],
        Some(config.as_bytes()),
    )?;
    crate::pipeline::pin_helper(&mut repo, "0.1.96")?;
    let jobs = crate::yaml_tree::document(&workflow(&repo)?)?["jobs"].clone();
    let (job, setup_id) = jobs
        .as_object()
        .into_iter()
        .flatten()
        .find_map(|(_, job)| {
            job["steps"]
                .as_array()?
                .iter()
                .find(|step| step["name"] == "Prepare a")
                .and_then(|step| step["id"].as_str().map(str::to_owned))
                .map(|id| (job.clone(), id))
        })
        .ok_or_else(|| format!("the setup of a is missing: {jobs}"))?;
    let steps = job["steps"].as_array().ok_or("the job has no steps")?;
    let waits_for_setup = format!("steps.{setup_id}.outcome == 'success'");
    let root_tests: Vec<&serde_json::Value> = steps
        .iter()
        .filter(|step| {
            let run = step["run"].as_str().unwrap_or_default();
            run.contains("tools/a") && run.contains("nextest") && !run.contains("--no-run")
        })
        .collect();
    assert!(!root_tests.is_empty(), "no test step of tools/a: {job}");
    for test in root_tests {
        let condition = test["if"].as_str().unwrap_or_default();
        assert!(
            condition.contains(&waits_for_setup),
            "a test of tools/a runs after its setup fails: {test}"
        );
    }
    Ok(())
}
