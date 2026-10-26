// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use tempfile::TempDir;
use velnor_actions_contract::{
    BUN_JOB_LANES_ENV, ExecutionMode, NAMED_CHECK_LANES_ENV, StepKind, WorkflowExpansion,
    expand_workflow,
};
use velnor_actions_orchestrator::{
    PlanOutputMode, REQUEST_FILE_ENV, migrate_config, plan_internal, plan_outputs, prepare,
    write_request,
};

use super::bun_routing::make_bun_repo;
use super::workflow_configuration_fixture::git_fixture;

const BUN_PLAN_JOBS: [&str; 4] = [
    "bun-fixture-ui",
    "bun-fixture-web",
    "bun-fixture-web--unit",
    "bun-fixture-web--browser",
];
const WRITE_REQUEST_CHILD_ENV: &str = "VELNOR_TEST_BUN_LANE_REQUEST_CHILD";
const PLAN_RESPONSE_FILE_ENV: &str = "VELNOR_TEST_BUN_LANE_RESPONSE_FILE";

#[test]
fn public_push_projects_and_checks_supplied_bun_lanes() -> Result<(), Box<dyn Error>> {
    if std::env::var_os(WRITE_REQUEST_CHILD_ENV).is_some() {
        write_request_and_plan()?;
        return Ok(());
    }
    let (repo, head) = make_single_push_repo()?;
    let prepared = prepare(repo.path())?;
    let execution = prepared
        .config
        .execution
        .as_ref()
        .ok_or("schema-2 fixture has no execution configuration")?;
    assert_eq!(execution.mode, None);
    assert_eq!(execution.default_profile, "hosted");
    let default = expand_workflow(&prepared.workflow.ir, &prepared.config, None)?;
    let (response_json, response) = plan(repo.path(), &head, None)?;
    assert_plan_jobs(&response, false)?;
    assert_eq!(
        plan_outputs(&response_json, PlanOutputMode::Static)?.not_affected_jobs,
        ""
    );

    let both = expand_workflow(
        &prepared.workflow.ir,
        &prepared.config,
        Some(ExecutionMode::Both),
    )?;
    let (paired_json, paired_response) = plan(repo.path(), &head, Some(&both.bun_job_lanes))?;
    assert_plan_jobs(&paired_response, true)?;
    assert_eq!(
        plan_outputs(&paired_json, PlanOutputMode::Static)?.not_affected_jobs,
        ""
    );
    assert_writer_transports_map(repo.path(), &head, &both)?;
    assert_invalid_lane_maps(repo.path(), &head, &both.bun_job_lanes)?;

    let scale = expand_workflow(
        &prepared.workflow.ir,
        &prepared.config,
        Some(ExecutionMode::ScaleSet),
    )?;
    let (scale_json, scale_response) = plan(repo.path(), &head, Some(&scale.bun_job_lanes))?;
    assert_plan_jobs(&scale_response, false)?;
    assert_eq!(
        plan_outputs(&scale_json, PlanOutputMode::Static)?.not_affected_jobs,
        ""
    );
    assert!(default.bun_job_lanes.values().all(|ids| ids.len() == 1));
    Ok(())
}

fn make_single_push_repo() -> Result<(TempDir, String), Box<dyn Error>> {
    let repo = make_bun_repo()?;
    migrate_config(repo.path(), 2, true)?;
    git(repo.path(), &["branch", "-m", "main"])?;
    git(repo.path(), &["add", "-A"])?;
    git(repo.path(), &["commit", "-m", "full push fixture"])?;
    let head = git(repo.path(), &["rev-parse", "HEAD"])?;
    Ok((repo, head))
}

fn assert_writer_transports_map(
    root: &Path,
    head: &str,
    expansion: &WorkflowExpansion,
) -> Result<(), Box<dyn Error>> {
    let lanes = &expansion.bun_job_lanes;
    let lane_json = emitted_bun_lane_json(expansion)?;
    let runner_temp = tempfile::tempdir()?;
    let request_path = runner_temp.path().join("plan-v1-request.json");
    let response_path = runner_temp.path().join("response.json");
    let event_path = runner_temp.path().join("event.json");
    fs::write(
        &event_path,
        json!({"before": "0000000000000000000000000000000000000000", "after": head}).to_string(),
    )?;
    let output = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "bun_request_lanes::public_push_projects_and_checks_supplied_bun_lanes",
            "--nocapture",
        ])
        .current_dir(root)
        .env(WRITE_REQUEST_CHILD_ENV, "1")
        .env(PLAN_RESPONSE_FILE_ENV, &response_path)
        .env(REQUEST_FILE_ENV, &request_path)
        .env("RUNNER_TEMP", runner_temp.path())
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_EVENT_PATH", &event_path)
        .env("GITHUB_SHA", head)
        .env("GITHUB_RUN_ID", "731948")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("VELNOR_RUN_KEY_ATTEMPT", "1")
        .env(BUN_JOB_LANES_ENV, lane_json)
        .env_remove(NAMED_CHECK_LANES_ENV)
        .env_remove("GITHUB_REPOSITORY")
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "write-request child failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let request: Value = serde_json::from_slice(&fs::read(request_path)?)?;
    assert_eq!(
        request["bun_job_lanes"],
        serde_json::to_value(lanes)?,
        "Bun writer must preserve the emitted lane map in request.bun_job_lanes"
    );
    let response_json = fs::read_to_string(response_path)?;
    let response = serde_json::from_str(&response_json)?;
    assert_plan_jobs(&response, true)?;
    assert_eq!(
        plan_outputs(&response_json, PlanOutputMode::Static)?.not_affected_jobs,
        ""
    );
    Ok(())
}

fn emitted_bun_lane_json(expansion: &WorkflowExpansion) -> Result<String, Box<dyn Error>> {
    let plan = expansion
        .ir
        .jobs
        .get("plan")
        .ok_or("expanded workflow has no plan job")?;
    let env = plan
        .steps
        .iter()
        .find_map(|step| match &step.kind {
            StepKind::Internal { operation, env } if operation == "write-request-v1:plan-v1" => {
                Some(env)
            }
            _ => None,
        })
        .ok_or("expanded plan job has no public plan request step")?;
    let lane_json = env
        .get(BUN_JOB_LANES_ENV)
        .ok_or("expanded plan request omits Bun lane map")?;
    let lanes: BTreeMap<String, Vec<String>> = serde_json::from_str(lane_json)?;
    assert_eq!(lanes, expansion.bun_job_lanes);
    Ok(lane_json.clone())
}

fn write_request_and_plan() -> Result<(), Box<dyn Error>> {
    let request_path = write_request()?;
    let request = fs::read_to_string(request_path)?;
    let response = plan_internal(&request)?;
    let response_path = std::env::var_os(PLAN_RESPONSE_FILE_ENV)
        .ok_or("write-request child has no response path")?;
    fs::write(response_path, response)?;
    Ok(())
}

fn plan(
    root: &Path,
    head: &str,
    bun_job_lanes: Option<&BTreeMap<String, Vec<String>>>,
) -> Result<(String, Value), Box<dyn Error>> {
    let mut request = json!({
        "schema": 1,
        "op": "plan-v1",
        "run_key": "r731948-a1",
        "event": "push",
        "head": head,
        "root": root.to_string_lossy(),
    });
    if let Some(lanes) = bun_job_lanes {
        request["bun_job_lanes"] = serde_json::to_value(lanes)?;
    }
    let response = plan_internal(&request.to_string())?;
    let parsed = serde_json::from_str(&response)?;
    Ok((response, parsed))
}

fn assert_plan_jobs(response: &Value, paired: bool) -> Result<(), Box<dyn Error>> {
    let jobs = response["plan"]["jobs"]
        .as_array()
        .ok_or("plan response has no job dispositions")?;
    let actual: BTreeMap<String, String> = jobs
        .iter()
        .filter_map(|job| {
            let id = job["job_id"].as_str()?;
            let disposition = job["disposition"].as_str()?;
            id.starts_with("bun-")
                .then(|| (id.to_owned(), disposition.to_owned()))
        })
        .collect();
    let expected: BTreeSet<String> = BUN_PLAN_JOBS
        .iter()
        .flat_map(|id| {
            if paired {
                vec![format!("{id}__hosted"), format!("{id}__local")]
            } else {
                vec![(*id).to_owned()]
            }
        })
        .collect();
    assert_eq!(
        actual.keys().cloned().collect::<BTreeSet<_>>(),
        expected,
        "Bun request dispatch must preserve paired physical job identities (paired={paired})"
    );
    assert!(actual.values().all(|disposition| disposition == "run"));
    assert!(!actual.keys().any(|id| id.contains("--browser--report")));
    Ok(())
}

fn assert_invalid_lane_maps(
    root: &Path,
    head: &str,
    expected: &BTreeMap<String, Vec<String>>,
) -> Result<(), Box<dyn Error>> {
    let mut missing = expected.clone();
    if missing.remove("bun-fixture-web--browser").is_none() {
        return Err("expanded map has no browser route".into());
    }
    let mut extra = expected.clone();
    extra.insert(
        "bun-fixture-unused".to_owned(),
        vec!["bun-fixture-unused".to_owned()],
    );
    let mut duplicate = expected.clone();
    let duplicate_outputs = duplicate
        .get_mut("bun-fixture-ui")
        .ok_or("expanded map has no UI member route")?;
    let (first_output, remaining_outputs) = duplicate_outputs.split_at_mut(1);
    let first_output = first_output
        .first()
        .ok_or("expanded UI route has no physical output")?;
    remaining_outputs
        .first_mut()
        .ok_or("expanded UI route has fewer than two physical outputs")?
        .clone_from(first_output);
    let mut wrong = expected.clone();
    let wrong_output = wrong
        .get_mut("bun-fixture-ui")
        .ok_or("expanded map has no UI member route")?
        .get_mut(0)
        .ok_or("expanded UI route has no physical output")?;
    "bun-fixture-ui__unexpected".clone_into(wrong_output);

    for (label, lanes) in [
        ("missing logical route", missing),
        ("extra logical route", extra),
        ("duplicate physical output", duplicate),
        ("unknown physical output", wrong),
    ] {
        let Err(error) = plan(root, head, Some(&lanes)) else {
            return Err(format!("{label} was unexpectedly accepted").into());
        };
        assert_eq!(
            error.to_string(),
            "internal: bun_job_lane_contract_mismatch"
        );
    }
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = git_fixture::command(root)?.args(args).output()?;
    if !output.status.success() {
        return Err(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
