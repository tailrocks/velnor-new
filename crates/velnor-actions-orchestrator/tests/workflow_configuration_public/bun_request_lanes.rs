// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use tempfile::TempDir;
use velnor_actions_contract::{
    ExecutionMode, NAMED_CHECK_LANES_ENV, StepKind, expand_workflow_with_bun_plan,
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
const WRITE_REQUEST_CHILD_ENV: &str = "VELNOR_TEST_BUN_REQUEST_CHILD";
const PLAN_RESPONSE_FILE_ENV: &str = "VELNOR_TEST_BUN_RESPONSE_FILE";

#[test]
fn public_push_writer_request_reaches_plan_internal_without_lane_field()
-> Result<(), Box<dyn Error>> {
    if std::env::var_os(WRITE_REQUEST_CHILD_ENV).is_some() {
        write_request_and_plan()?;
        return Ok(());
    }
    let (repo, head) = make_single_push_repo()?;
    let prepared = prepare(repo.path())?;
    let expansion = expand_workflow_with_bun_plan(
        &prepared.workflow.ir,
        &prepared.config,
        Some(ExecutionMode::Both),
    )?;
    let plan_job = expansion
        .ir
        .jobs
        .get("plan")
        .ok_or("expanded workflow has no plan job")?;
    let env = plan_job
        .steps
        .iter()
        .find_map(|step| match &step.kind {
            StepKind::Internal { operation, env } if operation == "write-request-v1:plan-v1" => {
                Some(env)
            }
            _ => None,
        })
        .ok_or("plan job has no public request writer")?;
    assert!(!env.contains_key("VELNOR_BUN_JOB_LANES_JSON"));

    let (response_json, response) = plan(repo.path(), &head, None)?;
    assert_plan_jobs(&response)?;
    assert_eq!(
        plan_outputs(&response_json, PlanOutputMode::Static)?.not_affected_jobs,
        ""
    );
    assert_unsupported_lane_fields(repo.path(), &head)?;
    assert_writer_request_reaches_plan_internal(repo.path(), &head)?;
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

fn assert_writer_request_reaches_plan_internal(
    root: &Path,
    head: &str,
) -> Result<(), Box<dyn Error>> {
    let runner_temp = tempfile::tempdir()?;
    let request_path = runner_temp.path().join("plan-v1-request.json");
    let response_path = runner_temp.path().join("response.json");
    let event_path = runner_temp.path().join("event.json");
    fs::write(
        &event_path,
        json!({"before":"0000000000000000000000000000000000000000", "after":head}).to_string(),
    )?;
    let output = Command::new(std::env::current_exe()?)
        .args(["--exact", "bun_request_lanes::public_push_writer_request_reaches_plan_internal_without_lane_field", "--nocapture"])
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
        .env_remove("VELNOR_BUN_JOB_LANES_JSON")
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
    assert!(request.get("bun_job_lanes").is_none());
    let response_json = fs::read_to_string(response_path)?;
    let response = serde_json::from_str(&response_json)?;
    assert_plan_jobs(&response)?;
    assert_eq!(
        plan_outputs(&response_json, PlanOutputMode::Static)?.not_affected_jobs,
        ""
    );
    Ok(())
}

fn write_request_and_plan() -> Result<(), Box<dyn Error>> {
    let request_path = write_request()?;
    let request = fs::read_to_string(request_path)?;
    let response = plan_internal(&request)?;
    let response_path =
        std::env::var_os(PLAN_RESPONSE_FILE_ENV).ok_or("request child has no response path")?;
    fs::write(response_path, response)?;
    Ok(())
}

fn plan(root: &Path, head: &str, extra: Option<Value>) -> Result<(String, Value), Box<dyn Error>> {
    let mut request = json!({"schema":1,"op":"plan-v1","run_key":"r731948-a1","event":"push","head":head,"root":root.to_string_lossy()});
    if let Some(value) = extra {
        request["bun_job_lanes"] = value;
    }
    let response = plan_internal(&request.to_string())?;
    let parsed = serde_json::from_str(&response)?;
    Ok((response, parsed))
}

fn assert_unsupported_lane_fields(root: &Path, head: &str) -> Result<(), Box<dyn Error>> {
    let identity = json!({"bun-fixture-ui":["bun-fixture-ui"]});
    let missing = json!({});
    let extra = json!({"bun-fixture-ui":["bun-fixture-ui"],"bun-unused":["bun-unused"]});
    let duplicate = json!({"bun-fixture-ui":["bun-fixture-ui","bun-fixture-ui"]});
    let unknown = json!({"bun-fixture-ui":["bun-fixture-ui__unknown"]});
    for (label, value) in [
        ("valid identity", identity),
        ("missing", missing),
        ("extra", extra),
        ("duplicate", duplicate),
        ("unknown", unknown),
    ] {
        let Err(error) = plan(root, head, Some(value)) else {
            return Err(format!("unsupported lane field shape {label} accepted").into());
        };
        assert!(
            error.to_string().contains("unknown field"),
            "{label}: {error}"
        );
    }
    Ok(())
}

fn assert_plan_jobs(response: &Value) -> Result<(), Box<dyn Error>> {
    let jobs = response["plan"]["jobs"]
        .as_array()
        .ok_or("plan response has no job dispositions")?;
    let actual: BTreeSet<String> = jobs
        .iter()
        .filter_map(|job| {
            let id = job["job_id"].as_str()?;
            let disposition = job["disposition"].as_str()?;
            id.starts_with("bun-").then(|| {
                assert_eq!(disposition, "run");
                id.to_owned()
            })
        })
        .collect();
    assert_eq!(
        actual,
        BUN_PLAN_JOBS.iter().map(|id| (*id).to_owned()).collect()
    );
    assert!(!actual.iter().any(|id| id.contains("--report")));
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
