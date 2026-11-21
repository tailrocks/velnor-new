// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const SHARED: &str = "mbx_shared_store = true\n";
const RECORDED: &str = "tools_restored";
const SAVE_RECORDED: &str = "save_tools_node";
const NOT_COLD: &str = "steps.prepare.outputs.cold != 'true'";

fn workflow_for(
    config: &str,
    helper: &str,
) -> Result<Vec<(String, Value)>, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    crate::pipeline::pin_helper(&mut repo, helper)?;
    let yaml: Value = crate::yaml_tree::document(&workflow(&repo)?)?;
    Ok(yaml["jobs"]
        .as_object()
        .ok_or("jobs missing")?
        .iter()
        .map(|(id, job)| (id.clone(), job.clone()))
        .collect())
}

fn steps(job: &Value) -> Vec<&Value> {
    job["steps"].as_array().into_iter().flatten().collect()
}

fn named<'a>(job: &'a Value, name: &str) -> Option<&'a Value> {
    steps(job).into_iter().find(|step| step["name"] == name)
}

fn id_of<'a>(job: &'a Value, id: &str) -> Option<&'a Value> {
    steps(job).into_iter().find(|step| step["id"] == id)
}

fn report_plan(job: &Value) -> Result<Value, Box<dyn std::error::Error>> {
    let step = named(job, "Report cache telemetry").ok_or("no report step")?;
    let plans: Value = serde_json::from_str(
        step["env"]["VELNOR_REPORT_CACHES"]
            .as_str()
            .ok_or("no plan")?,
    )?;
    plans
        .as_array()
        .and_then(|plans| plans.iter().find(|plan| plan["family"] == "tools"))
        .cloned()
        .ok_or_else(|| "no tools plan".into())
}

#[test]
fn a_node_store_hit_is_what_the_cache_telemetry_reads_as_the_tools_restore() -> TestResult {
    let jobs = workflow_for(SHARED, "0.1.89")?;
    let mut lanes = 0;
    for (id, job) in &jobs {
        let Some(network) = named(job, "Restore the tools cache") else {
            continue;
        };
        lanes += 1;
        let recorded = id_of(job, RECORDED).ok_or_else(|| format!("{id}: no recorder"))?;
        let at = |step: &Value| steps(job).iter().position(|other| *other == step);
        assert_eq!(at(recorded), at(network).map(|at| at + 1), "{id}");
        let run = recorded["run"].as_str().ok_or("no run")?;
        assert!(run.contains("cache-primary-key"), "{id}");
        assert!(run.contains("cache-hit"), "{id}");
        assert_eq!(
            recorded["env"]["MATCHED"], "${{ steps.tools.outputs.cache-matched-key }}",
            "{id}"
        );
        assert!(
            recorded["env"]["NODE_HIT"]
                .as_str()
                .is_some_and(|hit| hit.contains("steps.toolslocal.outputs.hit == 'true'")),
            "{id}"
        );
        assert_eq!(report_plan(job)?["restore_step"], RECORDED, "{id}");
    }
    assert!(lanes > 0);
    Ok(())
}

#[test]
fn without_the_node_store_the_telemetry_reads_the_cache_action() -> TestResult {
    for (id, job) in workflow_for("", "0.1.89")? {
        assert!(id_of(&job, RECORDED).is_none(), "{id}");
        if named(&job, "Restore the tools cache").is_some() {
            assert_eq!(report_plan(&job)?["restore_step"], "tools", "{id}");
        }
    }
    Ok(())
}

#[test]
fn a_failed_node_store_save_reaches_the_cache_check_from_helper_0_1_100() -> TestResult {
    let mut saving = 0;
    for (id, job) in workflow_for(SHARED, "0.1.100")? {
        let Some(local) = named(&job, "Save the tools to the node store") else {
            continue;
        };
        saving += 1;
        assert_eq!(local["id"], "toolslocalsave", "{id}");
        assert_eq!(local["continue-on-error"], true, "{id}");
        let recorder = id_of(&job, SAVE_RECORDED).ok_or_else(|| format!("{id}: no recorder"))?;
        assert_eq!(recorder["continue-on-error"], true, "{id}");
        let condition = recorder["if"].as_str().ok_or("no if")?;
        assert!(
            condition.contains("steps.toolslocalsave.outcome == 'failure'"),
            "{id}"
        );
        assert_eq!(report_plan(&job)?["save_step"], SAVE_RECORDED, "{id}");
    }
    assert!(saving > 0);
    Ok(())
}

#[test]
fn helper_0_1_98_leaves_the_node_store_save_unchecked() -> TestResult {
    for (id, job) in workflow_for(SHARED, "0.1.98")? {
        if let Some(local) = named(&job, "Save the tools to the node store") {
            assert!(local.get("continue-on-error").is_none(), "{id}");
            assert!(local.get("id").is_none(), "{id}");
            assert!(id_of(&job, SAVE_RECORDED).is_none(), "{id}");
            assert_eq!(report_plan(&job)?["save_step"], "save_tools", "{id}");
        }
    }
    Ok(())
}

fn condition_of(job: &Value, name: &str) -> String {
    named(job, name)
        .and_then(|step| step["if"].as_str())
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn a_cold_run_saves_no_base_from_helper_0_1_100() -> TestResult {
    let mut saves = 0;
    for (id, job) in workflow_for("", "0.1.100")? {
        let condition = condition_of(&job, "Save the target cache");
        if condition.is_empty() {
            continue;
        }
        saves += 1;
        assert!(condition.contains(NOT_COLD), "{id}: {condition}");
        let record = condition_of(&job, "Record the target cache key");
        assert!(record.contains(NOT_COLD), "{id}: {record}");
    }
    assert!(saves > 0);
    Ok(())
}

#[test]
fn helper_0_1_98_never_reads_the_cold_output() -> TestResult {
    for (id, job) in workflow_for("", "0.1.98")? {
        assert!(
            !condition_of(&job, "Save the target cache").contains("cold"),
            "{id}"
        );
    }
    Ok(())
}

#[test]
fn archive_runners_prime_the_node_store_from_the_network_cache() -> TestResult {
    let config = format!("shape = \"groups\"\n{SHARED}");
    let jobs = workflow_for(&config, "0.1.89")?;
    let mut runners = 0;
    for (id, job) in &jobs {
        if named(job, "Download the tests").is_none() {
            continue;
        }
        let prime = named(job, "Prime the node store with the tools")
            .ok_or_else(|| format!("{id}: archive runner does not prime"))?;
        runners += 1;
        assert_eq!(prime["continue-on-error"], true, "{id}");
        let condition = prime["if"].as_str().ok_or("no if")?;
        assert!(
            condition.contains("steps.toolslocal.outputs.hit != 'true'"),
            "{id}"
        );
        assert!(
            condition.contains("steps.tools.outputs.cache-hit == 'true'"),
            "{id}"
        );
        assert!(condition.contains("github.event_name == 'push'"), "{id}");
        let at = |name: &str| steps(job).iter().position(|step| step["name"] == name);
        assert!(
            at("Prime the node store with the tools") < at("Install tools"),
            "{id}"
        );
    }
    assert!(runners > 0);
    Ok(())
}

#[test]
fn helper_0_1_99_has_neither_the_cold_output_nor_the_checked_save() -> TestResult {
    for (id, job) in workflow_for(SHARED, "0.1.99")? {
        assert!(
            !condition_of(&job, "Save the target cache").contains("cold"),
            "{id}"
        );
        assert!(id_of(&job, SAVE_RECORDED).is_none(), "{id}");
        assert!(id_of(&job, "toolslocalsave").is_none(), "{id}");
    }
    Ok(())
}
