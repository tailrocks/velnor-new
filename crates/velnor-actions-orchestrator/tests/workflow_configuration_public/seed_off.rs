// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;

use serde_json::{Value, json};
use velnor_actions_contract::{Job, StepKind};
use velnor_actions_orchestrator::{
    GenerationPreparation, finalized_jobs, plan_internal, prepare, render_staged_tree,
};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::workflow_configuration_fixture::{self, git_fixture};

const OFF: &str = "\n[stacks.rust]\ncompiled_state = \"off\"\n";
const PRODUCE: &str = "\n[stacks.rust]\ncompiled_state = \"slices\"\n";

pub(super) fn seed_repo(extra: &str) -> Result<tempfile::TempDir, Box<dyn Error>> {
    let mut config = workflow_configuration_fixture::config_with_verification_branch(None);
    config.push_str(extra);
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    let root = repo.path();
    fs::write(
        root.join("mise.toml"),
        format!(
            "[tools]\nrust = \"{}\"\n",
            velnor_actions_mise::RUST_VERSION
        ),
    )?;
    for (relative_root, package_name) in [("crates/a-b", "a-b"), ("crates/a/c", "c")] {
        workflow_configuration_fixture::add_independent_mbx_package(
            root,
            relative_root,
            package_name,
        )?;
    }
    Ok(repo)
}

fn seed_ids(prepared: &GenerationPreparation) -> BTreeSet<&str> {
    prepared
        .workflow
        .ir
        .jobs
        .iter()
        .filter(|(_, job)| {
            job.producer_kind()
                == Some(velnor_actions_contract::workflow::ProducerKind::StateProducer)
        })
        .map(|(id, _)| id.as_str())
        .collect()
}

#[test]
fn prepare_and_render_emit_no_seed_jobs_when_compiled_state_is_off() -> Result<(), Box<dyn Error>> {
    let produce_repo = seed_repo("")?;
    let produce = prepare(produce_repo.path())?;
    let off_repo = seed_repo(OFF)?;
    let off = prepare(off_repo.path())?;

    let produced = seed_ids(&produce);
    assert_eq!(produced.len(), 2);
    assert!(seed_ids(&off).is_empty());

    let remaining: Vec<&str> = produce
        .workflow
        .ir
        .jobs
        .keys()
        .map(String::as_str)
        .filter(|id| !produced.contains(id))
        .collect();
    assert_eq!(
        remaining,
        off.workflow
            .ir
            .jobs
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
    for id in remaining {
        let left: &Job = &produce.workflow.ir.jobs[id];
        let right: &Job = &off.workflow.ir.jobs[id];
        assert_eq!(
            velnor_actions_contract::canonical_json_bytes(left)?,
            velnor_actions_contract::canonical_json_bytes(right)?,
            "{id}"
        );
    }
    let rendered = render_staged_tree(&off)?;
    let workflow = rendered.get(WORKFLOW_PATH).ok_or("workflow missing")?;
    assert!(!workflow.contains("state-producer"));
    assert!(!workflow.contains("State producer"));
    Ok(())
}

#[test]
fn explicit_produce_renders_the_same_tree_as_the_default() -> Result<(), Box<dyn Error>> {
    let default_repo = seed_repo("")?;
    let explicit_repo = seed_repo(PRODUCE)?;
    let default = prepare(default_repo.path())?;
    let explicit = prepare(explicit_repo.path())?;
    assert_eq!(seed_ids(&default).len(), 2);
    assert_eq!(seed_ids(&default), seed_ids(&explicit));
    let left = render_staged_tree(&default)?;
    let right = render_staged_tree(&explicit)?;
    assert_eq!(left.paths(), right.paths());
    for path in left.paths() {
        assert_eq!(left.get(&path), right.get(&path), "{path}");
    }
    Ok(())
}

#[test]
fn controller_plan_accepts_a_seedless_graph() -> Result<(), Box<dyn Error>> {
    let repo = seed_repo(OFF)?;
    let root = repo.path();
    workflow_configuration_fixture::git(&["add", "-A"], root)?;
    workflow_configuration_fixture::git(&["commit", "-qm", "base"], root)?;
    let head = String::from_utf8(
        git_fixture::command(root)?
            .args(["rev-parse", "HEAD"])
            .output()?
            .stdout,
    )?;
    let request = json!({
        "schema": 1,
        "op": "plan-v1",
        "run_key": "r190001-a1",
        "head": head.trim(),
        "event": "push",
        "root": root.to_string_lossy(),
    });
    let response: Value = serde_json::from_str(&plan_internal(&request.to_string())?)?;
    let jobs = response
        .get("plan")
        .and_then(|plan| plan.get("jobs"))
        .and_then(Value::as_array)
        .filter(|jobs| !jobs.is_empty())
        .ok_or("plan response has no jobs")?;
    for job in jobs {
        let id = job
            .get("job_id")
            .and_then(Value::as_str)
            .ok_or("plan job has no id")?;
        assert!(!id.contains("state-producer"), "{id}");
    }
    Ok(())
}

#[test]
fn seeding_names_the_clippy_and_doc_lanes_and_never_the_doctest_lane() -> Result<(), Box<dyn Error>>
{
    let repo = seed_repo("\n[stacks.rust]\nseed_parallel_targets = true\n")?;
    for root in [".", "crates/a-b", "crates/a/c"] {
        workflow_configuration_fixture::write_nextest_task(&repo.path().join(root))?;
    }
    let prepared = prepare(repo.path())
        .map_err(|error| format!("seed lanes were rejected at generation: {error}"))?;
    let jobs = finalized_jobs(&prepared)?;
    let mut seeded = 0;
    for (id, crate_job) in jobs.iter().filter(|(id, _)| id.starts_with("rust-")) {
        let doctests = crate_job.steps.iter().any(|step| {
            step.name == "Doctests" && matches!(step.kind, StepKind::TaskExecution { .. })
        });
        for step in &crate_job.steps {
            let StepKind::Internal { env, .. } = &step.kind else {
                continue;
            };
            let Some(lanes) = env.get("VELNOR_TARGET_LANES") else {
                continue;
            };
            assert!(doctests, "{id} is seeded without a Doctests lane to refuse");
            assert_eq!(lanes, "velnor-clippy,velnor-doc", "{id}");
            seeded += 1;
        }
    }
    assert!(seeded > 0, "no crate job was seeded");
    Ok(())
}

#[test]
fn controller_plan_lists_no_moved_job_and_executes_every_producer_lane()
-> Result<(), Box<dyn Error>> {
    let repo = seed_repo("")?;
    let root = repo.path();
    let prepared = prepare(root)?;
    let combined = velnor_actions_orchestrator::finalized_jobs(&prepared)?;
    workflow_configuration_fixture::git(&["add", "-A"], root)?;
    workflow_configuration_fixture::git(&["commit", "-qm", "base"], root)?;
    let head = String::from_utf8(
        git_fixture::command(root)?
            .args(["rev-parse", "HEAD"])
            .output()?
            .stdout,
    )?;
    let request = json!({
        "schema": 1,
        "op": "plan-v1",
        "run_key": "r190001-a1",
        "head": head.trim(),
        "event": "push",
        "root": root.to_string_lossy(),
    });
    let response: Value = serde_json::from_str(&plan_internal(&request.to_string())?)?;
    let plan = response.get("plan").ok_or("plan response has no plan")?;
    let listed: BTreeSet<&str> = plan
        .get("jobs")
        .and_then(Value::as_array)
        .ok_or("plan has no jobs")?
        .iter()
        .filter_map(|job| job.get("job_id").and_then(Value::as_str))
        .collect();
    for moved in combined
        .iter()
        .filter(|(_, job)| job.producer_kind().is_some())
        .map(|(id, _)| id.as_str())
    {
        assert!(!listed.contains(moved), "plan-v1 lists {moved}");
    }
    let executed: BTreeSet<&str> = plan
        .get("obligations")
        .and_then(Value::as_array)
        .ok_or("plan has no obligations")?
        .iter()
        .filter(|obligation| obligation.get("decision").and_then(Value::as_str) == Some("execute"))
        .filter_map(|obligation| obligation.get("task_id").and_then(Value::as_str))
        .collect();
    let mut lanes = 0;
    for (id, job) in &combined {
        if !id.starts_with("state-producer-") {
            continue;
        }
        for step in &job.steps {
            if let velnor_actions_contract::StepKind::TaskExecution { task_id, .. } = &step.kind {
                lanes += 1;
                assert!(executed.contains(task_id.as_str()), "{id}: {task_id}");
            }
        }
    }
    assert_ne!(lanes, 0, "no producer lane was checked");
    Ok(())
}
