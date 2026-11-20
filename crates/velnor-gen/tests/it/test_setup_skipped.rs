// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};
use crate::test_groups::view_with;
use crate::yaml_tree::document;

const GROUP: &str = "[[workspace_groups]]\nname = 'tools'\nroots = ['tools/a', 'tools/b']\n\
    [[test_setup]]\nroot = 'tools/b'\nname = 'Prepare b'\nphase = 'build'\nrun = 'bash scripts/b.sh'\n";

fn grouped(config: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut repo = view(
        &["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"],
        Some(config.as_bytes()),
    )?;
    pin_helper(&mut repo, "0.1.110")?;
    Ok(document(&workflow(&repo)?)?["jobs"].clone())
}

fn runs_after(condition: &str, setup_id: &str, outcome: &str) -> bool {
    let waits = format!("steps.{setup_id}.outcome");
    if condition.contains(&format!("{waits} == 'success'")) {
        return outcome == "success";
    }
    !condition.contains(&format!("{waits} != '{outcome}'"))
}

fn steps_of(job: &serde_json::Value) -> Vec<serde_json::Value> {
    job["steps"].as_array().cloned().unwrap_or_default()
}

#[test]
fn a_setup_skipped_by_its_mode_gate_does_not_stop_the_other_roots_cargo_steps() -> TestResult {
    let jobs = grouped(GROUP)?;
    let mut checked = 0;
    for (id, job) in jobs.as_object().into_iter().flatten() {
        let steps = steps_of(job);
        let Some(setup) = steps.iter().find(|step| step["name"] == "Prepare b") else {
            continue;
        };
        let setup_id = setup["id"].as_str().ok_or("the setup has no id")?;
        assert!(
            setup["if"]
                .as_str()
                .is_some_and(|text| text.contains("_mode") && text.contains("'none'")),
            "{id}: the setup has no mode gate: {setup}"
        );
        let position = steps
            .iter()
            .position(|step| step["name"] == "Prepare b")
            .ok_or("the setup moved")?;
        for step in steps.iter().skip(position + 1).filter(|step| {
            let run = step["run"].as_str().unwrap_or_default();
            run.contains("tools/a") && run.contains("cargo") && !run.contains("--no-run")
        }) {
            checked += 1;
            let condition = step["if"].as_str().unwrap_or_default();
            assert!(
                runs_after(condition, setup_id, "skipped"),
                "{id}: a cargo step of tools/a waits for the skipped setup of tools/b: {step}"
            );
            for outcome in ["failure", "cancelled"] {
                assert!(
                    !runs_after(condition, setup_id, outcome),
                    "{id}: a cargo step of tools/a runs after the setup of tools/b ended {outcome}: {step}"
                );
            }
        }
    }
    assert!(
        checked > 0,
        "no test of tools/a shares a job with the setup of tools/b: {jobs}"
    );
    Ok(())
}

#[test]
fn a_check_configurations_step_waits_for_its_roots_setup() -> TestResult {
    let config = "[[check_configurations]]\nroot = '.'\n\
        [[test_setup]]\nroot = '.'\nname = 'Prepare'\nrun = 'bash scripts/prepare.sh'\n";
    let repo = view_with(&["Cargo.toml", "tools/Cargo.toml"], config, "0.1.110")?;
    let jobs = document(&workflow(&repo)?)?["jobs"].clone();
    let mut checked = 0;
    for (id, job) in jobs.as_object().into_iter().flatten() {
        let steps = steps_of(job);
        let Some(setup) = steps.iter().find(|step| step["name"] == "Prepare") else {
            continue;
        };
        let setup_id = setup["id"].as_str().ok_or("the setup has no id")?;
        for step in steps
            .iter()
            .filter(|step| step["name"] == "velnor-ci check-configurations")
        {
            checked += 1;
            let condition = step["if"].as_str().unwrap_or_default();
            assert!(runs_after(condition, setup_id, "skipped"), "{id}: {step}");
            for outcome in ["failure", "cancelled"] {
                assert!(
                    !runs_after(condition, setup_id, outcome),
                    "{id}: check-configurations runs after the setup ended {outcome}: {step}"
                );
            }
        }
    }
    assert!(
        checked > 0,
        "no check-configurations step was rendered: {jobs}"
    );
    Ok(())
}
