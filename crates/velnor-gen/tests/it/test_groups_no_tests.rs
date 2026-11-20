// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::test_groups::{SHELL, job, step, view_with};
use crate::yaml_tree::document;

const TEST_STEP: &str = "Test";

fn rendered_on(config: &str, helper: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(document(&workflow(&view_with(
        &["Cargo.toml"],
        config,
        helper,
    )?)?)?)
}

fn group_run(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let workflow = rendered_on(config, "0.1.108")?;
    let group = job(&workflow, "rust-test-shell")?;
    Ok(step(group, TEST_STEP)?["run"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

#[test]
fn an_unwritten_no_tests_key_makes_a_group_fail_when_it_selects_nothing() -> TestResult {
    let run = group_run(SHELL)?;
    assert!(run.ends_with("--no-tests=fail"), "{run}");
    assert!(!run.contains("--no-tests=pass"), "{run}");
    let workflow = rendered_on(SHELL, "0.1.108")?;
    let default = step(job(&workflow, "rust-test-default")?, TEST_STEP)?["run"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(default.ends_with("--no-tests=pass"), "{default}");
    Ok(())
}

#[test]
fn each_written_no_tests_value_renders_as_its_nextest_flag() -> TestResult {
    for mode in ["fail", "warn", "pass"] {
        let run = group_run(&format!("{SHELL}no_tests = '{mode}'\n"))?;
        assert!(
            run.ends_with(&format!("--no-tests={mode}")),
            "{mode}: {run}"
        );
    }
    Ok(())
}

#[test]
fn an_unknown_no_tests_value_is_rejected_with_the_key_and_the_value() -> TestResult {
    for bad in ["always", "Fail", "", "none"] {
        let error = workflow(&view_with(
            &["Cargo.toml"],
            &format!("{SHELL}no_tests = '{bad}'\n"),
            "0.1.108",
        )?)
        .err()
        .ok_or_else(|| format!("accepted no_tests = {bad:?}"))?
        .to_string();
        assert!(
            error.contains("test_groups.no_tests") && error.contains(&format!("{bad:?}")),
            "{bad}: {error}"
        );
    }
    Ok(())
}

#[test]
fn no_tests_needs_a_helper_that_reads_the_key_even_for_the_default_value() -> TestResult {
    for config in [
        format!("{SHELL}no_tests = 'pass'\n"),
        format!("{SHELL}no_tests = 'fail'\n"),
    ] {
        let error = workflow(&view_with(&["Cargo.toml"], &config, "0.1.107")?)
            .err()
            .ok_or("accepted on a helper that does not read the key")?
            .to_string();
        assert!(
            error.contains("no_tests") && error.contains("0.1.108"),
            "{error}"
        );
    }
    workflow(&view_with(&["Cargo.toml"], SHELL, "0.1.107")?)?;
    Ok(())
}
