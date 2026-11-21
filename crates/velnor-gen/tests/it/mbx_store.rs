// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
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

const SEED: &str = "\"$VELNOR_EXEC\" mbx-seed";

#[test]
fn the_pr_seed_runs_after_the_plan_and_only_for_a_selection_and_never_in_the_planning_job()
-> TestResult {
    let text = workflow(&repo("mbx_shared_store = true\n", "0.1.86")?)?;
    let yaml: serde_json::Value = crate::yaml_tree::document(&text)?;
    let jobs = yaml["jobs"].as_object().ok_or("jobs missing")?;
    let mut seeded = 0;
    for (id, job) in jobs {
        let steps = job["steps"].as_array().ok_or("steps missing")?;
        let at = |needle: &str| {
            steps
                .iter()
                .position(|s| s["run"].as_str().is_some_and(|run| run.contains(needle)))
        };
        if id == "rust-plan" {
            assert!(
                at(EXEC).is_none() && at(SEED).is_none(),
                "{id}: the planning job seeds nothing"
            );
            continue;
        }
        let Some(store) = at(EXEC) else { continue };
        let seed = at(SEED).ok_or_else(|| format!("{id}: no mbx-seed"))?;
        assert_eq!(seed, store + 1, "{id}");
        let condition = steps[seed]["if"].as_str().ok_or("seed has no if")?;
        assert!(
            condition.contains(
                "github.event_name == 'pull_request' || github.event_name == 'merge_group'"
            ),
            "{id}: {condition}"
        );
        if let Some(plan) = steps.iter().position(|s| s["id"].as_str() == Some("plan")) {
            assert!(
                plan < store,
                "{id}: the plan runs before the store and the seed"
            );
            assert!(
                condition.contains("steps.plan.outputs.rust_"),
                "{id}: {condition}"
            );
            assert!(condition.contains("_mode != 'none'"), "{id}: {condition}");
        }
        seeded += 1;
    }
    assert!(seeded > 0);
    Ok(())
}

#[test]
fn an_older_helper_keeps_phase_one_without_a_seed_step() -> TestResult {
    let text = workflow(&repo("mbx_shared_store = true\n", "0.1.85")?)?;
    assert!(text.contains("mbx-store"));
    assert!(!text.contains("mbx-seed"));
    assert!(!workflow(&repo("", "0.1.86")?)?.contains("mbx-seed"));
    Ok(())
}

#[test]
fn a_job_that_builds_nothing_runs_no_shared_store_steps_from_helper_0_1_112() -> TestResult {
    let steps_of = |helper: &str| -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let mut repo = crate::golden::load(&crate::golden::samples().join("policy-lane"))?;
        repo.files.insert(
            path(".velnor/rust-ci.toml")?,
            b"mbx_shared_store = true\n".to_vec(),
        );
        crate::pipeline::pin_helper(&mut repo, helper)?;
        let text = workflow(&repo)?;
        let yaml: serde_json::Value = crate::yaml_tree::document(&text)?;
        let steps = yaml["jobs"]["policy"]["steps"]
            .as_array()
            .ok_or("policy steps missing")?;
        Ok(steps
            .iter()
            .filter_map(|s| s["run"].as_str())
            .filter(|run| run.contains(EXEC) || run.contains(SEED))
            .map(str::to_owned)
            .collect())
    };
    assert_eq!(steps_of("0.1.111")?.len(), 2);
    assert_eq!(steps_of("0.1.112")?, Vec::<String>::new());
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
    assert!(runs.iter().any(|run| run.contains(SEED)), "{runs:?}");
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
                assert!(step.get("if").is_some(), "{id}: mbx-store runs without a selection gate");
                stores += 1;
            }
        }
    }
    assert!(stores > 0);
    Ok(())
}
