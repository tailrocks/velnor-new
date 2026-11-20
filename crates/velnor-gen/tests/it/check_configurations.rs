// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::test_groups::view_with;
use crate::yaml_tree::document;

const ROOTS: &[&str] = &["Cargo.toml", "tools/Cargo.toml"];
const HELPER: &str = "0.1.110";
const STEP: &str = "velnor-ci check-configurations";
const KEY: &str =
    "[[check_configurations]]\nroot = '.'\nconfigurations = 'ci/configurations.json'\n";

fn jobs_of(config: &str, helper: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let text = workflow(&view_with(ROOTS, config, helper)?)?;
    Ok(document(&text)?["jobs"].clone())
}

fn refusal(config: &str, helper: &str) -> Result<String, Box<dyn std::error::Error>> {
    let error = workflow(&view_with(ROOTS, config, helper)?)
        .err()
        .ok_or("the configuration was accepted")?;
    Ok(error.to_string())
}

fn found(jobs: &serde_json::Value) -> Vec<(String, usize, Option<usize>, serde_json::Value)> {
    let mut found = Vec::new();
    for (id, job) in jobs.as_object().into_iter().flatten() {
        let steps = job["steps"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let tests = steps.iter().position(|step| step["id"] == "test");
        for (index, step) in steps.iter().enumerate() {
            if step["name"] == STEP {
                found.push((id.clone(), index, tests, step.clone()));
            }
        }
    }
    found
}

#[test]
fn without_the_key_no_job_calls_check_configurations() -> TestResult {
    assert_eq!(found(&jobs_of("", HELPER)?).len(), 0);
    Ok(())
}

#[test]
fn the_key_renders_one_step_in_the_test_job_of_its_root_before_the_tests() -> TestResult {
    let steps = found(&jobs_of(KEY, HELPER)?);
    assert_eq!(steps.len(), 1, "{steps:?}");
    let (id, index, tests, step) = &steps[0];
    assert_eq!(id, "rust-root");
    assert!(tests.is_some_and(|tests| index < &tests), "{steps:?}");
    assert_eq!(
        step["run"],
        ". \"$VELNOR_EXEC\" check-configurations --configurations ci/configurations.json"
    );
    assert!(step["working-directory"].is_null(), "{step}");
    Ok(())
}

#[test]
fn the_configurations_file_is_optional_and_a_nested_root_runs_in_its_folder() -> TestResult {
    let steps = found(&jobs_of(
        "[[check_configurations]]\nroot = 'tools'\n",
        HELPER,
    )?);
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert_eq!(steps[0].0, "rust-tools");
    assert_eq!(steps[0].3["run"], ". \"$VELNOR_EXEC\" check-configurations");
    assert_eq!(steps[0].3["working-directory"], "tools");
    Ok(())
}

#[test]
fn a_helper_older_than_the_subcommand_is_refused_with_the_key_and_release() -> TestResult {
    let text = refusal(KEY, "0.1.109")?;
    assert!(text.contains("check_configurations"), "{text}");
    assert!(text.contains("0.1.110"), "{text}");
    jobs_of(KEY, HELPER)?;
    Ok(())
}

#[test]
fn an_unknown_root_or_an_unusable_file_path_is_refused_with_its_name() -> TestResult {
    for (config, mention) in [
        ("[[check_configurations]]\nroot = 'missing'\n", "missing"),
        ("[[check_configurations]]\n", "root"),
        (
            "[[check_configurations]]\nroot = '.'\nconfigurations = '../x.json'\n",
            "check_configurations.configurations",
        ),
        (
            "[[check_configurations]]\nroot = '.'\nconfigurations = 'a b.json'\n",
            "check_configurations.configurations",
        ),
    ] {
        let text = refusal(config, HELPER)?;
        assert!(text.contains(mention), "{config}: {text}");
    }
    Ok(())
}
