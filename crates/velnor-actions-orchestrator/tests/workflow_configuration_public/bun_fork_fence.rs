// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::process::Command;

use serde_json::Value;
use velnor_actions_orchestrator::{ExecutionMode, prepare, render_staged_tree_with};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::bun_planning_fixture::{ChangedMember, make_bun_planning_repo_with_scopes};

#[test]
fn private_bun_jobs_fail_closed_before_checkout_on_fork_events() -> Result<(), Box<dyn Error>> {
    for changed in [ChangedMember::Web, ChangedMember::SuiteOnly] {
        let repo = make_bun_planning_repo_with_scopes(changed, true)?;
        let prepared = prepare(repo.repo.path())?;
        for mode in [
            ExecutionMode::Hosted,
            ExecutionMode::ScaleSet,
            ExecutionMode::Both,
        ] {
            let rendered = render_staged_tree_with(&prepared, Some(mode))?;
            let workflow: Value =
                serde_saphyr::from_str(rendered.get(WORKFLOW_PATH).ok_or("missing workflow")?)?;
            let jobs = workflow["jobs"].as_object().ok_or("missing jobs")?;
            let mut expected = vec![
                "bun-fixture-ui",
                "bun-fixture-web--unit",
                "bun-fixture-web--browser",
                "bun-fixture-web--browser--report",
            ];
            if matches!(changed, ChangedMember::Web) {
                expected.push("bun-fixture-web");
            }
            for id in expected {
                let job = jobs
                    .get(id)
                    .ok_or_else(|| format!("missing private job: {id}"))?;
                let steps = job["steps"].as_array().ok_or("missing steps")?;
                let first = &steps[0];
                assert_eq!(
                    first["name"], "Reject private registry access on fork pull requests",
                    "{id}"
                );
                assert!(
                    first.get("if").is_none(),
                    "guard must run unconditionally: {id}"
                );
                assert_eq!(
                    first["env"]["VELNOR_PULL_REQUEST_FORK"],
                    "${{ toJSON(github.event.pull_request.head.repo.fork) }}"
                );
                let command = first["run"].as_str().ok_or("guard command missing")?;
                for (event, fork, expected) in [
                    ("pull_request", "false", true),
                    ("pull_request", "true", false),
                    ("pull_request", "null", false),
                    ("pull_request", "\"false\"", false),
                    ("pull_request", "", false),
                    ("pull_request", "garbage", false),
                    ("push", "null", true),
                    ("merge_group", "null", true),
                    ("workflow_dispatch", "null", true),
                    ("", "null", false),
                ] {
                    let status = Command::new("sh")
                        .args(["-e", "-c", command])
                        .env("GITHUB_EVENT_NAME", event)
                        .env("VELNOR_PULL_REQUEST_FORK", fork)
                        .status()?;
                    assert_eq!(
                        status.success(),
                        expected,
                        "{id}: event={event}, fork={fork}"
                    );
                }
            }
        }
    }
    let repo = make_bun_planning_repo_with_scopes(ChangedMember::Web, false)?;
    let rendered = render_staged_tree_with(&prepare(repo.repo.path())?, None)?;
    let workflow = rendered.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    assert!(!workflow.contains("Reject private registry access on fork pull requests"));
    Ok(())
}
