// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use serde_json::{Value, json};
use velnor_actions_contract::RESTRICTED_ACTION_EVENTS;
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_WORKFLOW_PATH;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::cache_prune::CACHE_PRUNE_PATH;
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::one_job_per_crate::mbx_repo;
use super::state_prune_yaml::{evaluate, parse};

const CI_NAME: &str = "Pipeline";
const DEFAULT_BRANCH: &str = "testmain";

struct Rendered {
    ci: String,
    sealed: String,
    prune: String,
    workflow_run_files: Vec<String>,
}

fn render() -> Result<Rendered, Box<dyn Error>> {
    let repo = mbx_repo(false)?;
    let config_path = repo.path().join(".velnor/config.toml");
    let config = fs::read_to_string(&config_path)?;
    fs::write(
        &config_path,
        config.replacen("name = \"CI\"\n", &format!("name = \"{CI_NAME}\"\n"), 1),
    )?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let text = |path: &str| -> Result<String, Box<dyn Error>> {
        Ok(tree
            .get(path)
            .ok_or(format!("{path} not generated"))?
            .to_owned())
    };
    let workflow_run_files = tree
        .paths()
        .into_iter()
        .filter(|path| {
            tree.get(path)
                .is_some_and(|body| body.lines().any(|line| line == "  workflow_run:"))
        })
        .collect();
    Ok(Rendered {
        ci: text(WORKFLOW_PATH)?,
        sealed: text(STATE_SEAL_WORKFLOW_PATH)?,
        prune: text(CACHE_PRUNE_PATH)?,
        workflow_run_files,
    })
}

fn payload(event: &str, head_branch: &str, head_repository: &str, conclusion: &str) -> Value {
    json!({
        "github": {
            "repository": "acme/app",
            "event_name": "workflow_run",
            "event": {
                "workflow_run": {
                    "event": event,
                    "head_branch": head_branch,
                    "head_repository": { "full_name": head_repository },
                    "conclusion": conclusion,
                }
            }
        }
    })
}

fn guard_rows() -> Vec<(&'static str, Value, bool)> {
    vec![
        (
            "push on main, success",
            payload("push", DEFAULT_BRANCH, "acme/app", "success"),
            true,
        ),
        (
            "push on main, failure",
            payload("push", DEFAULT_BRANCH, "acme/app", "failure"),
            true,
        ),
        (
            "pull_request from main of the same repo",
            payload("pull_request", DEFAULT_BRANCH, "acme/app", "success"),
            false,
        ),
        (
            "push on a feature branch",
            payload("push", "feature", "acme/app", "success"),
            false,
        ),
        (
            "fork pull_request whose branch is main",
            payload("pull_request", DEFAULT_BRANCH, "fork/app", "success"),
            false,
        ),
        (
            "push whose head repository is another repo",
            payload("push", DEFAULT_BRANCH, "fork/app", "success"),
            false,
        ),
        (
            "pull_request_target from a fork",
            payload("pull_request_target", DEFAULT_BRANCH, "fork/app", "success"),
            false,
        ),
        (
            "cancelled push on main",
            payload("push", DEFAULT_BRANCH, "acme/app", "cancelled"),
            false,
        ),
        (
            "workflow_dispatch",
            payload("workflow_dispatch", DEFAULT_BRANCH, "acme/app", "success"),
            false,
        ),
        (
            "schedule",
            payload("schedule", DEFAULT_BRANCH, "acme/app", "success"),
            false,
        ),
    ]
}

#[test]
fn prune_runs_only_after_a_main_push_ci() -> Result<(), Box<dyn Error>> {
    let rendered = render()?;
    let ci_name = rendered
        .ci
        .lines()
        .find_map(|line| line.strip_prefix("name: "))
        .ok_or("ci.yml has no top-level name")?;
    assert_eq!(ci_name, CI_NAME);
    let sealed_name = rendered
        .sealed
        .lines()
        .find_map(|line| line.strip_prefix("name: "))
        .ok_or("state-seal.yml has no top-level name")?;

    let workflow = parse(&rendered.prune)?;
    assert_eq!(
        workflow["on"],
        json!({
            "workflow_run": {
                "workflows": [ci_name, sealed_name],
                "types": ["completed"],
                "branches": [DEFAULT_BRANCH],
            }
        })
    );
    assert_eq!(workflow["permissions"], json!({}));
    assert_eq!(
        workflow["concurrency"],
        json!({"group": "velnor-cache-prune", "cancel-in-progress": false})
    );
    let job = &workflow["jobs"]["prune"];
    assert_eq!(
        job["permissions"],
        json!({"actions": "write", "attestations": "read"})
    );
    let steps = job["steps"].as_array().ok_or("prune job has no steps")?;
    assert!(
        steps.iter().all(|step| step.get("uses").is_none()),
        "prune must not run any action or checkout: {steps:?}"
    );

    let guard = job["if"].as_str().ok_or("prune job has no if guard")?;
    for (name, context, runs) in guard_rows() {
        assert_eq!(evaluate(guard, &context)?, runs, "{name}: guard {guard}");
    }
    Ok(())
}

#[test]
fn only_the_prune_workflow_listens_for_workflow_run() -> Result<(), Box<dyn Error>> {
    let rendered = render()?;
    assert!(RESTRICTED_ACTION_EVENTS.contains(&"workflow_run"));
    assert_eq!(rendered.workflow_run_files, [CACHE_PRUNE_PATH]);
    Ok(())
}
