// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::test_groups::view_with;
use crate::yaml_tree::document;

const CHECK: &str = "velnor-ci check-configurations";

fn single_root_gates_its_test_on_the_check(helper: &str) -> TestResult {
    let view = view_with(
        &["Cargo.toml"],
        "[[check_configurations]]\nroot = '.'\n",
        helper,
    )?;
    let jobs = document(&workflow(&view)?)?["jobs"].clone();
    let steps = jobs
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(_, job)| job["steps"].as_array())
        .find(|steps| steps.iter().any(|step| step["name"] == CHECK))
        .ok_or("no job runs the check")?;
    let id = steps
        .iter()
        .find(|step| step["name"] == CHECK)
        .and_then(|step| step["id"].as_str())
        .ok_or("no check step")?;
    let test = steps
        .iter()
        .find(|step| step["id"] == "test")
        .and_then(|step| step["if"].as_str())
        .ok_or("no test condition")?;
    assert!(
        test.contains(&format!("steps.{id}.outcome != 'failure'")),
        "{helper}: {test}"
    );
    Ok(())
}

#[test]
fn a_single_root_gates_its_test_step_on_the_check_at_the_released_pin_0_1_110() -> TestResult {
    single_root_gates_its_test_on_the_check("0.1.110")
}

#[test]
fn a_single_root_gates_its_test_step_on_the_check_at_the_released_pin_0_1_111() -> TestResult {
    single_root_gates_its_test_on_the_check("0.1.111")
}

#[test]
fn a_single_root_gates_its_test_step_on_the_check_at_0_1_112() -> TestResult {
    single_root_gates_its_test_on_the_check("0.1.112")
}
