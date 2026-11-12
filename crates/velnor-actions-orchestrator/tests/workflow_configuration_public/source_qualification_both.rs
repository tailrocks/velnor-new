// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use serde_json::{Value, json};
use velnor_actions_contract::{ExecutionMode, expand_workflow_with_lane_plan};
use velnor_actions_orchestrator::{plan_internal, prepare, render_staged_tree};

use super::scoped_workspace::{commit, put};
use super::source_qualification::WORKFLOW_PATH;
use super::source_qualification_selection::{fixture, install_selector, run_selector};

#[test]
fn source_qualification_projects_both_real_cli_lanes() -> Result<(), Box<dyn Error>> {
    let (repo, _) = fixture()?;
    let root = repo.path();
    let config_path = root.join(".velnor/config.toml");
    let config = fs::read_to_string(&config_path)?;
    fs::write(
        &config_path,
        config.replace("[execution]\n", "[execution]\nmode = \"both\"\n"),
    )?;
    let prepared = prepare(root)?;
    let expanded = expand_workflow_with_lane_plan(
        &prepared.workflow.ir,
        &prepared.config,
        Some(ExecutionMode::Both),
    )?;
    let inventory =
        serde_json::to_value(expanded.lane_plan.as_ref().ok_or("lane plan")?.inventory())?;
    let tree = render_staged_tree(&prepared)?;
    let workflow: Value = serde_saphyr::from_str(tree.get(WORKFLOW_PATH).ok_or("workflow")?)?;
    let selector = workflow["jobs"]["selection"]["steps"]
        .as_array()
        .ok_or("selection")?
        .iter()
        .find(|step| step["name"] == "Admit source qualification selection")
        .ok_or("selector")?;
    let routes: Vec<String> =
        serde_json::from_str(selector["env"]["CLI_ROUTES"].as_str().ok_or("routes")?)?;
    assert_eq!(
        routes,
        [
            "rust-velnor-actions-cli__hosted",
            "rust-velnor-actions-cli__local"
        ]
    );
    let command = selector["run"].as_str().ok_or("selector command")?;
    install_selector(root)?;
    let mut base = commit(root, "both lanes")?;
    for (path, expected, disposition) in [
        ("docs/guide.md", "skip", "not_affected"),
        ("fixtures/oracle.txt", "run", "run"),
    ] {
        put(root, path, "changed\n")?;
        let head = commit(root, "change")?;
        let request = json!({"schema": 1, "op": "plan-v1", "run_key": "r190001-a1", "event": "pull_request", "base": base, "head": head, "root": root, "lane_inventory": inventory});
        let response: Value = serde_json::from_str(&plan_internal(&request.to_string())?)?;
        let plan = &response["plan"];
        for id in [
            "rust-velnor-actions-cli__hosted",
            "rust-velnor-actions-cli__local",
        ] {
            let job = plan["jobs"]
                .as_array()
                .ok_or("jobs")?
                .iter()
                .find(|job| job["job_id"] == id)
                .ok_or("CLI lane")?;
            assert_eq!(job["lane"]["logical_job_id"], "rust-velnor-actions-cli");
            assert_eq!(job["disposition"], disposition);
        }
        let (success, output) = run_selector(root, command, plan, "")?;
        assert!(success);
        assert_eq!(output.trim(), format!("decision={expected}"));
        if expected == "run" {
            for mutation in [
                "job_missing",
                "job_duplicate",
                "lane_wrong",
                "lane_disposition",
            ] {
                assert!(
                    !run_selector(root, command, plan, mutation)?.0,
                    "{mutation}"
                );
            }
        }
        base = head;
    }
    Ok(())
}
