// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::TestResult;
use crate::job_trust_boundary::violations;
use crate::pipeline::{pin_helper, view, workflow};
use crate::private_sources::{SECTION, private_repo};
use crate::test_groups::view_with;
use crate::yaml_tree::document;

const NAME: &str = "Prepare the fixture";
const SETUP: &str = "[[test_setup]]\nroot = '.'\nname = 'Prepare the fixture'\nrun = 'bash scripts/prepare-fixture.sh'\n";
const SHAPES: [&str; 3] = ["one-job", "lint-test", "groups"];

fn build_phase() -> String {
    format!("{SETUP}phase = 'build'\n")
}

fn rendered(config: &str, helper: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(document(&workflow(&view_with(
        &["Cargo.toml"],
        config,
        helper,
    )?)?)?)
}

fn steps(job: &Value) -> &[Value] {
    job["steps"].as_array().map_or(&[], Vec::as_slice)
}

fn compiles(step: &Value) -> bool {
    let run = step["run"].as_str().unwrap_or_default();
    run.contains("cargo")
        && !["cargo-fmt", "cargo-machete", "cargo-deny"]
            .iter()
            .any(|tool| run.contains(tool))
}

fn setups(job: &Value) -> Vec<usize> {
    steps(job)
        .iter()
        .enumerate()
        .filter(|(_, step)| step["name"] == NAME)
        .map(|(index, _)| index)
        .collect()
}

fn jobs(workflow: &Value) -> Vec<(&String, &Value)> {
    workflow["jobs"].as_object().into_iter().flatten().collect()
}

#[test]
fn a_build_phase_setup_runs_before_the_first_compiling_step_of_every_job_that_compiles()
-> TestResult {
    for shape in SHAPES {
        let workflow = rendered(&format!("shape = '{shape}'\n{}", build_phase()), "0.1.109")?;
        let mut checked = Vec::new();
        for (id, job) in jobs(&workflow) {
            let first = steps(job).iter().position(compiles);
            let found = setups(job);
            let Some(first) = first else {
                assert!(
                    found.is_empty(),
                    "{shape} {id}: setup in a job with no cargo"
                );
                continue;
            };
            assert_eq!(found.len(), 1, "{shape} {id}: {found:?}");
            assert_eq!(
                found[0] + 1,
                first,
                "{shape} {id}: setup is not just before cargo"
            );
            checked.push(id.as_str());
        }
        let wanted: &[&str] = match shape {
            "one-job" => &["rust"],
            "lint-test" => &["rust-lint", "rust-test"],
            _ => &["rust-lint", "rust-test", "rust-test-default"],
        };
        assert_eq!(checked, wanted, "{shape}");
    }
    Ok(())
}

#[test]
fn the_build_phase_setup_has_the_gate_of_the_lint_step_it_precedes() -> TestResult {
    let workflow = rendered(
        &format!("shape = 'lint-test'\n{}", build_phase()),
        "0.1.109",
    )?;
    let lint = &workflow["jobs"]["rust-lint"];
    let setup = &steps(lint)[setups(lint)[0]];
    let clippy = steps(lint)
        .iter()
        .find(|step| step["name"] == "Lint")
        .ok_or("no Lint step")?;
    assert_eq!(setup["if"], clippy["if"]);
    assert!(setup["if"].as_str().is_some(), "{setup}");
    Ok(())
}

#[test]
fn without_the_key_or_with_the_test_phase_the_setup_still_runs_only_before_the_tests() -> TestResult
{
    for shape in SHAPES {
        let plain = rendered(&format!("shape = '{shape}'\n{SETUP}"), "0.1.109")?;
        let test = rendered(
            &format!("shape = '{shape}'\n{SETUP}phase = 'test'\n"),
            "0.1.109",
        )?;
        assert_eq!(plain, test, "{shape}");
        for (id, job) in jobs(&plain) {
            let found = setups(job);
            let Some(&setup) = found.first() else {
                continue;
            };
            let first_test = steps(job)
                .iter()
                .position(|step| {
                    step["run"]
                        .as_str()
                        .is_some_and(|run| run.contains("nextest run"))
                })
                .ok_or("no test step")?;
            assert_eq!(setup + 1, first_test, "{shape} {id}");
        }
        if let Some(lint) = plain["jobs"].get("rust-lint") {
            assert!(setups(lint).is_empty(), "{shape}: lint job");
        }
    }
    Ok(())
}

#[test]
fn a_build_phase_setup_of_a_grouped_root_runs_before_that_roots_first_compiling_step() -> TestResult {
    const GROUP: &str = "[[workspace_groups]]\nname = \"tools\"\nroots = [\"tools/a\", \"tools/b\"]\n";
    const ROOTS: [&str; 3] = ["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"];
    for shape in ["one-job", "lint-test"] {
        let config = format!(
            "shape = '{shape}'\n{GROUP}[[test_setup]]\nroot = 'tools/a'\nname = '{NAME}'\nrun = 'bash scripts/prepare-fixture.sh'\nphase = 'build'\n"
        );
        let mut repo = view(&ROOTS, Some(config.as_bytes()))?;
        pin_helper(&mut repo, "0.1.109")?;
        let parsed = document(&workflow(&repo)?)?;
        let mut checked = 0;
        for (id, job) in jobs(&parsed) {
            let compiles_a = |step: &Value| {
                compiles(step)
                    && step["run"]
                        .as_str()
                        .is_some_and(|run| run.contains("tools/a/Cargo.toml"))
            };
            match steps(job).iter().position(compiles_a) {
                None => assert!(setups(job).is_empty(), "{shape} {id}: setup without a root a step"),
                Some(first) => {
                    checked += 1;
                    assert_eq!(setups(job), vec![first - 1], "{shape} {id}");
                }
            }
        }
        assert!(checked > 0, "{shape}: no job compiled tools/a");
    }
    Ok(())
}

#[test]
fn a_setup_of_another_root_stays_out_of_this_roots_jobs() -> TestResult {
    let config = build_phase().replace("root = '.'", "root = 'tools'");
    let workflow = document(&workflow(&view_with(
        &["Cargo.toml", "tools/Cargo.toml"],
        &config,
        "0.1.109",
    )?)?)?;
    for (id, job) in jobs(&workflow) {
        assert_eq!(
            !setups(job).is_empty(),
            id.contains("tools") && steps(job).iter().any(compiles),
            "{id}"
        );
    }
    Ok(())
}

#[test]
fn private_sources_put_the_token_and_the_fetch_before_a_build_phase_setup() -> TestResult {
    for shape in SHAPES.into_iter().filter(|shape| *shape != "groups") {
        let config = format!("shape = '{shape}'\n{SECTION}{}", build_phase());
        let text = workflow(&private_repo(&["Cargo.toml"], &config, "0.1.109")?)?;
        let parsed = document(&text)?;
        let mut checked = 0;
        for (id, job) in jobs(&parsed) {
            let Some(&setup) = setups(job).first() else {
                continue;
            };
            checked += 1;
            let at = |wanted: &dyn Fn(&Value) -> bool| steps(job).iter().position(wanted);
            let token = at(&|step| step["id"] == "private-token").ok_or("no token step")?;
            let fetch = at(&|step| {
                step["run"]
                    .as_str()
                    .is_some_and(|run| run.contains("\"$VELNOR_EXEC\" fetch "))
            })
            .ok_or("no fetch step")?;
            let first = steps(job).iter().position(compiles).ok_or("no cargo")?;
            assert!(
                token < fetch && fetch < setup && setup < first,
                "{shape} {id}: {token} {fetch} {setup} {first}"
            );
            assert!(
                !steps(job)[setup].to_string().contains("private-token"),
                "the setup command must not see the token"
            );
        }
        assert_eq!(checked, if shape == "one-job" { 1 } else { 2 }, "{shape}");
        let bare = format!("shape = '{shape}'\n{SECTION}");
        let without = workflow(&private_repo(&["Cargo.toml"], &bare, "0.1.109")?)?;
        assert_eq!(violations(&text)?, violations(&without)?, "{shape}");
    }
    Ok(())
}

#[test]
fn the_phase_key_needs_a_helper_that_reads_it_and_a_known_value() -> TestResult {
    for config in [build_phase(), format!("{SETUP}phase = 'test'\n")] {
        let error = workflow(&view_with(&["Cargo.toml"], &config, "0.1.107")?)
            .err()
            .ok_or("accepted on a helper that does not read the key")?
            .to_string();
        assert!(
            error.contains("phase") && error.contains("0.1.109"),
            "{error}"
        );
    }
    workflow(&view_with(&["Cargo.toml"], SETUP, "0.1.90")?)?;
    let error = workflow(&view_with(
        &["Cargo.toml"],
        &format!("{SETUP}phase = 'lint'\n"),
        "0.1.109",
    )?)
    .err()
    .ok_or("an unknown phase was accepted")?
    .to_string();
    assert!(
        error.contains("test_setup.phase") && error.contains("lint"),
        "{error}"
    );
    Ok(())
}
