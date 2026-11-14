// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use serde_json::json;
use velnor_actions_orchestrator::{
    PlanOutputMode, plan_outputs, prepare, render_staged_tree_with, write_request_parts,
};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::support::rendered_job_block;

use super::bun_planning_fixture::{
    BunPlanningRepo, ChangedMember, make_bun_planning_repo, make_bun_planning_repo_with_scopes,
};
use super::scoped_workspace::{PlanRun, merge_verdict, miss_reasons, plan_request, verdict_passed};

const REQUIRED_LOGICAL_JOBS: [&str; 4] = [
    "bun-fixture-ui",
    "bun-fixture-web",
    "bun-fixture-web--unit",
    "bun-fixture-web--browser",
];

#[test]
fn configured_bun_both_projects_single_job_dispositions_into_required() -> Result<(), Box<dyn Error>>
{
    let repo = make_bun_planning_repo(ChangedMember::Web)?;
    assert_ne!(
        repo.head, repo.merge,
        "the fixture must preserve a separate pull request head"
    );
    let run = plan(&repo, "r190001-a1")?;
    assert_eq!(
        run.jobs()?,
        expected_jobs("not_affected", "run"),
        "a web-only change must keep the independent UI member skipped"
    );
    let outputs = plan_outputs(&run.response, PlanOutputMode::Static)?;
    assert_eq!(
        outputs.not_affected_jobs,
        ",bun-fixture-ui,bun-fixture-ui--assets,"
    );
    let passed = verdict(&run, Some("bun-fixture-ui"), None)?;
    assert!(verdict_passed(&passed)?);
    let skipped = verdict(&run, Some("bun-fixture-ui"), Some("bun-fixture-web"))?;
    assert!(!verdict_passed(&skipped)?);
    assert!(miss_reasons(&skipped).contains("planned_job_skipped"));
    Ok(())
}

#[test]
fn bun_workspace_dependency_runs_selected_workspace_members() -> Result<(), Box<dyn Error>> {
    let repo = make_bun_planning_repo(ChangedMember::Ui)?;
    let run = plan(&repo, "r190002-a1")?;
    assert_eq!(
        run.jobs()?,
        expected_jobs("run", "run"),
        "changing the workspace dependency must select its web consumer"
    );
    let outputs = plan_outputs(&run.response, PlanOutputMode::Static)?;
    assert!(
        outputs.not_affected_jobs.is_empty(),
        "an affected workspace dependency must run every Bun output"
    );
    let passed = verdict(&run, None, None)?;
    assert!(verdict_passed(&passed)?);
    let skipped = verdict(&run, None, Some("bun-fixture-web--browser"))?;
    assert!(!verdict_passed(&skipped)?);
    assert!(miss_reasons(&skipped).contains("planned_job_skipped"));
    Ok(())
}

fn plan(repo: &BunPlanningRepo, run_key: &str) -> Result<PlanRun, Box<dyn Error>> {
    plan_request(&json!({
        "schema": 1,
        "op": "plan-v1",
        "run_key": run_key,
        "base": repo.base.as_str(),
        "head": repo.head.as_str(),
        "event": "pull_request",
        "root": repo.repo.path().to_string_lossy(),
    }))
}

fn verdict(
    run: &PlanRun,
    skipped_member: Option<&str>,
    skipped_job: Option<&str>,
) -> Result<serde_json::Value, Box<dyn Error>> {
    let conclusions: BTreeMap<String, &str> = REQUIRED_LOGICAL_JOBS
        .iter()
        .map(|id| {
            let skipped = skipped_member.is_some_and(|member| id.starts_with(member))
                || skipped_job == Some(*id);
            (
                (*id).to_owned(),
                if skipped { "skipped" } else { "success" },
            )
        })
        .collect();
    merge_verdict(run, "pull_request", &conclusions)
}

fn expected_jobs(ui: &str, web: &str) -> BTreeMap<String, String> {
    let mut expected = BTreeMap::new();
    for (logical_id, disposition) in [
        ("bun-fixture-ui", ui),
        ("bun-fixture-ui--assets", ui),
        ("bun-fixture-web", web),
        ("bun-fixture-web--unit", web),
        ("bun-fixture-web--browser", web),
        ("bun-fixture-web--assets", web),
    ] {
        expected.insert(logical_id.to_owned(), disposition.to_owned());
    }
    expected
}

#[test]
fn fork_private_registry_admission_follows_selected_bun_work() -> Result<(), Box<dyn Error>> {
    for (changed, private, fork, expected) in [
        (ChangedMember::Web, true, true, None),
        (ChangedMember::Web, true, false, Some("run")),
        (ChangedMember::Web, false, true, Some("run")),
        (ChangedMember::Docs, true, true, Some("not_affected")),
    ] {
        let repo = make_bun_planning_repo_with_scopes(changed, private)?;
        let rendered = render_staged_tree_with(&prepare(repo.repo.path())?, None)?;
        let workflow = rendered.get(WORKFLOW_PATH).ok_or("missing workflow")?;
        let job = rendered_job_block(workflow, "bun-fixture-web")?;
        assert_eq!(
            job.contains("Install dependencies (registry phase)"),
            private
        );
        assert_eq!(job.contains("packages: read"), private);
        if !private {
            assert!(!job.contains("packages:"));
        }
        assert!(
            job.contains("!contains(needs.plan.outputs.not_affected_jobs, ',bun-fixture-web,')")
        );
        let request_dir = tempfile::tempdir()?;
        let request_path = request_dir.path().join("plan-v1-request.json");
        let payload = json!({"pull_request": {"base": {"sha": repo.base}, "head": {"sha": repo.head, "repo": {"fork": fork}}}});
        write_request_parts(
            &request_path,
            "pull_request",
            &payload.to_string(),
            None,
            None,
            request_dir.path(),
        )?;
        let mut request: serde_json::Value = serde_json::from_slice(&fs::read(request_path)?)?;
        assert_eq!(request["event"], if fork { "fork" } else { "pull_request" });
        request["root"] = json!(repo.repo.path());
        request["run_key"] = json!("r190003-a1");
        match expected {
            None => {
                let error = plan_request(&request)
                    .err()
                    .ok_or("fork private registry plan unexpectedly succeeded")?;
                assert!(
                    error
                        .to_string()
                        .contains("private_npm_registry_forbidden_for_fork"),
                    "{error}"
                );
            }
            Some(disposition) => {
                let run = plan_request(&request).map_err(|err| {
                    format!("case private={private} fork={fork} expected={disposition}: {err}")
                })?;
                assert_eq!(
                    run.jobs()?.get("bun-fixture-web").map(String::as_str),
                    Some(disposition)
                );
                if matches!(changed, ChangedMember::Docs) {
                    assert!(
                        run.jobs()?
                            .iter()
                            .filter(|(id, _)| id.starts_with("bun-"))
                            .all(|(_, value)| value == "not_affected")
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn private_suite_only_is_rejected_for_selected_fork_work() -> Result<(), Box<dyn Error>> {
    let repo = make_bun_planning_repo_with_scopes(ChangedMember::SuiteOnly, true)?;
    let rendered = render_staged_tree_with(&prepare(repo.repo.path())?, None)?;
    let workflow = rendered.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    assert!(rendered_job_block(workflow, "bun-fixture-web").is_err());
    let suite = rendered_job_block(workflow, "bun-fixture-web--browser")?;
    assert!(suite.contains("Install dependencies (registry phase)"));
    assert!(suite.contains("packages: read"));

    for fork in [false, true] {
        let request_dir = tempfile::tempdir()?;
        let path = request_dir.path().join("plan-v1-request.json");
        let payload = json!({"pull_request": {"base": {"sha": repo.base}, "head": {"sha": repo.head, "repo": {"fork": fork}}}});
        write_request_parts(
            &path,
            "pull_request",
            &payload.to_string(),
            None,
            None,
            request_dir.path(),
        )?;
        let mut request: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
        assert_eq!(request["event"], if fork { "fork" } else { "pull_request" });
        request["root"] = json!(repo.repo.path());
        request["run_key"] = json!("r190004-a1");
        if fork {
            let error = plan_request(&request)
                .err()
                .ok_or("fork private suite plan unexpectedly succeeded")?;
            assert!(
                error
                    .to_string()
                    .contains("private_npm_registry_forbidden_for_fork"),
                "{error}"
            );
        } else {
            let jobs = plan_request(&request)?.jobs()?;
            assert!(!jobs.contains_key("bun-fixture-web"));
            assert_eq!(
                jobs.get("bun-fixture-web--browser").map(String::as_str),
                Some("run")
            );
        }
    }
    Ok(())
}
