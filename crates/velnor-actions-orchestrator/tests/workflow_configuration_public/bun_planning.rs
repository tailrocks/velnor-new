// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use serde_json::json;
use velnor_actions_orchestrator::{PlanOutputMode, plan_outputs};

use super::bun_planning_fixture::{BunPlanningRepo, ChangedMember, make_bun_planning_repo};
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
