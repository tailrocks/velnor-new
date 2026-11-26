// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::gate_eval::{Context, runs};
use crate::pipeline::{view, workflow};

const EXEC: &str = "\"$VELNOR_EXEC\" mbx-store";

fn repo(config: &str, helper: &str) -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
    repo.files
        .insert(path(".velnor/rust-ci.toml")?, config.as_bytes().to_vec());
    crate::pipeline::pin_helper(&mut repo, helper)?;
    Ok(repo)
}

#[test]
fn the_shared_store_is_selected_once_per_job_before_any_cargo_or_tool_step() -> TestResult {
    for shape in ["", "shape = 'groups'\n"] {
        let text = workflow(&repo(
            &format!("{shape}mbx_shared_store = true\n"),
            "0.1.85",
        )?)?;
        let yaml: serde_json::Value = crate::yaml_tree::document(&text)?;
        let jobs = yaml["jobs"].as_object().ok_or("jobs missing")?;
        let mut cargo_jobs = 0;
        for (id, job) in jobs {
            let steps = job["steps"].as_array().ok_or("steps missing")?;
            let run = |step: &serde_json::Value, needle: &str| {
                step["run"].as_str().is_some_and(|run| run.contains(needle))
            };
            let selected: Vec<_> = steps
                .iter()
                .enumerate()
                .filter(|(_, s)| run(s, EXEC))
                .collect();
            assert!(selected.len() <= 1, "{id}");
            let first_work = steps.iter().position(|step| {
                run(step, "\"$VELNOR_EXEC\" run ")
                    || step["uses"]
                        .as_str()
                        .is_some_and(|uses| uses.starts_with("jdx/mise-action@"))
            });
            if let Some(work) = first_work {
                cargo_jobs += 1;
                let (at, step) = selected
                    .first()
                    .ok_or_else(|| format!("{id}: no mbx-store"))?;
                assert!(*at < work, "{id}");
                assert!(
                    step.get("if").is_none(),
                    "{id}: runs on every event, the helper decides"
                );
            }
        }
        assert!(cargo_jobs > 0);
    }
    Ok(())
}

#[test]
fn the_shared_store_is_off_unless_configured() -> TestResult {
    assert!(!workflow(&repo("", "0.1.84")?)?.contains("mbx-store"));
    assert!(!workflow(&repo("mbx_shared_store = false\n", "0.1.85")?)?.contains("mbx-store"));
    Ok(())
}

#[test]
fn a_helper_that_cannot_select_the_store_is_refused() -> TestResult {
    assert!(
        workflow(&repo("mbx_shared_store = true\n", "0.1.84")?)
            .is_err_and(|error| error.to_string().contains("0.1.85"))
    );
    Ok(())
}

#[test]
fn a_cargo_capable_archive_job_keeps_the_shared_store_steps_from_helper_0_1_112() -> TestResult {
    let config = format!("mbx_shared_store = true\n{}", crate::test_groups::SHELL);
    let text = workflow(&crate::test_groups::view_with(
        &["Cargo.toml"],
        &config,
        "0.1.112",
    )?)?;
    let yaml: serde_json::Value = crate::yaml_tree::document(&text)?;
    let job = crate::test_groups::job(&yaml, "rust-test-shell")?;
    let runs: Vec<&str> = job["steps"]
        .as_array()
        .ok_or("steps missing")?
        .iter()
        .filter_map(|s| s["run"].as_str())
        .collect();
    assert!(runs.iter().any(|run| run.contains(EXEC)), "{runs:?}");
    Ok(())
}

#[test]
fn a_one_job_lane_gates_the_shared_store_on_its_selection_from_helper_0_1_112() -> TestResult {
    let text = workflow(&crate::test_groups::view_with(
        &["Cargo.toml"],
        "mbx_shared_store = true\n",
        "0.1.112",
    )?)?;
    let yaml: serde_json::Value = crate::yaml_tree::document(&text)?;
    let mut stores = 0;
    for (id, job) in yaml["jobs"].as_object().ok_or("jobs missing")? {
        for step in job["steps"].as_array().ok_or("steps missing")? {
            if step["run"].as_str().is_some_and(|run| run.contains(EXEC)) {
                assert!(
                    step.get("if").is_some(),
                    "{id}: mbx-store runs without a selection gate"
                );
                stores += 1;
            }
        }
    }
    assert!(stores > 0);
    Ok(())
}

fn one_root_store_step(helper: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml"], None)?;
    repo.files.insert(
        path(".velnor/rust-ci.toml")?,
        b"mbx_shared_store = true\n".to_vec(),
    );
    crate::pipeline::pin_helper(&mut repo, helper)?;
    let yaml: serde_json::Value = crate::yaml_tree::document(&workflow(&repo)?)?;
    let steps = yaml["jobs"]["rust"]["steps"]
        .as_array()
        .ok_or("the one-job workflow has no rust steps")?;
    steps
        .iter()
        .find(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.ends_with("mbx-store"))
        })
        .cloned()
        .ok_or_else(|| "the one-job workflow has no mbx-store".into())
}

fn store_condition(helper: &str) -> Result<String, Box<dyn std::error::Error>> {
    let condition = one_root_store_step(helper)?["if"]
        .as_str()
        .ok_or("mbx-store has no if")?
        .to_owned();
    condition
        .strip_prefix("${{ ")
        .and_then(|rest| rest.strip_suffix(" }}"))
        .map(str::to_owned)
        .ok_or_else(|| format!("unexpected if: {condition}").into())
}

#[test]
fn the_shared_store_is_skipped_on_the_no_work_path_from_0_1_112() -> TestResult {
    let condition = store_condition("0.1.112")?;
    let mut context = Context::new(&[
        ("steps.plan.outcome", "success"),
        ("steps.plan.outputs.rust_root_mode", "none"),
    ]);
    assert!(!runs(&condition, &context)?, "{condition}");
    context.set("steps.plan.outputs.rust_root_mode", "full");
    assert!(runs(&condition, &context)?, "{condition}");
    Ok(())
}

#[test]
fn an_older_helper_keeps_the_self_planning_store_ungated() -> TestResult {
    assert!(one_root_store_step("0.1.111")?.get("if").is_none());
    Ok(())
}

#[test]
fn the_shared_store_is_skipped_when_the_plan_failed_or_was_skipped_from_0_1_112() -> TestResult {
    let condition = store_condition("0.1.112")?;
    for outcome in ["failure", "skipped"] {
        let context = Context::new(&[
            ("steps.plan.outcome", outcome),
            ("steps.plan.outputs.rust_root_mode", ""),
        ]);
        assert!(!runs(&condition, &context)?, "{outcome}: {condition}");
    }
    Ok(())
}

#[test]
fn every_job_that_selects_the_shared_store_first_points_mbx_at_a_private_directory() -> TestResult {
    for helper in ["0.1.85", "0.1.111", "0.1.112"] {
        for shape in ["", "shape = 'groups'\n"] {
            let text = workflow(&repo(&format!("{shape}mbx_shared_store = true\n"), helper)?)?;
            let yaml: serde_json::Value = crate::yaml_tree::document(&text)?;
            let mut stores = 0;
            for (id, job) in yaml["jobs"].as_object().ok_or("jobs missing")? {
                let steps = job["steps"].as_array().ok_or("steps missing")?;
                let position =
                    |matches: &dyn Fn(&serde_json::Value) -> bool| steps.iter().position(matches);
                let Some(store) =
                    position(&|step| step["run"].as_str().is_some_and(|run| run.contains(EXEC)))
                else {
                    continue;
                };
                stores += 1;
                let private = position(&|step| step["name"] == "Private mbx store")
                    .ok_or_else(|| format!("{helper} {id}: no private mbx store"))?;
                assert!(
                    private < store,
                    "{helper} {id}: the shared store must override"
                );
                assert_eq!(
                    steps[private]["run"].as_str(),
                    Some("echo \"MBX_CACHE_DIR=$RUNNER_TEMP/mbx\" >> \"$GITHUB_ENV\""),
                    "{helper} {id}"
                );
            }
            assert!(stores > 0, "{helper}");
        }
    }
    Ok(())
}

#[test]
fn every_sample_job_with_the_shared_store_step_has_a_private_mbx_store_before_it() -> TestResult {
    let mut stores = 0;
    for dir in crate::golden::sample_dirs(&crate::golden::samples())? {
        let workflows = dir.join("expected/.github/workflows");
        let Ok(entries) = std::fs::read_dir(&workflows) else {
            continue;
        };
        for entry in entries {
            let file = entry?.path();
            let text = std::fs::read_to_string(&file)?;
            let yaml: serde_json::Value = crate::yaml_tree::document(&text)?;
            let Some(jobs) = yaml["jobs"].as_object() else {
                continue;
            };
            for (id, job) in jobs {
                let steps = job["steps"].as_array().map_or(&[][..], Vec::as_slice);
                let named = |name: &str| steps.iter().position(|step| step["name"] == name);
                let Some(store) = named("velnor-ci mbx-store") else {
                    continue;
                };
                stores += 1;
                let private = named("Private mbx store").ok_or_else(|| {
                    format!(
                        "{}: {file:?} job {id} has no private mbx store",
                        dir.display()
                    )
                })?;
                assert!(
                    private < store,
                    "{}: {} job {id}",
                    dir.display(),
                    file.display()
                );
            }
        }
    }
    assert!(stores > 0, "no sample selects the shared store");
    Ok(())
}
