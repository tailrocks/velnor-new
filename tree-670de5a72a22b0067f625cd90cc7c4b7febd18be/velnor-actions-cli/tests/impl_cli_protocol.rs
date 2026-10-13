//! Event-time protocol: write-request materialization, plan outputs, merge verdict.

use std::error::Error;
use std::path::Path;

use crate::impl_cli_tmp::{
    cleanup, code, commit_all, fresh_tempdir, git_init, head_sha, spawn_isolated,
};

/// Pin the push branch so plan works without origin/HEAD.
fn pin_branch(repo: &Path) -> Result<(), Box<dyn Error>> {
    let config = repo.join(".velnor").join("config.toml");
    let mut body = std::fs::read_to_string(&config)?;
    body.push_str("\n[workflow]\ndefault_branch = \"main\"\n");
    std::fs::write(&config, body)?;
    Ok(())
}

/// Assert stderr leaks no private op tag.
fn assert_no_leak(output: &std::process::Output) {
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!stderr.contains("plan-v1"), "leaked: {stderr}");
    assert!(!stderr.contains("merge-v1"), "leaked: {stderr}");
    assert!(!stderr.contains("write-request-v1"), "leaked: {stderr}");
    assert!(!stderr.contains("VELNOR_INTERNAL"), "leaked: {stderr}");
}

#[test]
fn write_request_materializes_push_request() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("proto-wr")?;
    let base = "b".repeat(64);
    let head = "a".repeat(64);
    let payload = tmp.join("event.json");
    std::fs::write(
        &payload,
        format!("{{\"before\":\"{base}\",\"after\":\"{head}\"}}"),
    )?;
    let request = tmp
        .join("velnor")
        .join("request")
        .join("plan-v1-request.json");
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "write-request-v1"),
            ("VELNOR_REQUEST_FILE", request.to_str().unwrap_or("/")),
            ("GITHUB_EVENT_NAME", "push"),
            ("GITHUB_EVENT_PATH", payload.to_str().unwrap_or("/")),
            ("GITHUB_SHA", &head),
            ("RUNNER_TEMP", tmp.to_str().unwrap_or("/")),
        ],
        &tmp,
    )?;
    assert_eq!(code(&output), 0);
    assert_eq!(output.stdout, [] as [u8; 0]);
    assert_no_leak(&output);
    let expected = format!(
        "{{\"base\":\"{base}\",\"event\":\"push\",\"head\":\"{head}\",\"op\":\"plan-v1\",\"root\":\".\",\"schema\":1}}"
    );
    assert_eq!(std::fs::read_to_string(&request)?, expected);
    cleanup(&tmp);
    Ok(())
}

#[test]
fn write_request_rejects_malformed_payload() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("proto-wr-bad")?;
    let payload = tmp.join("event.json");
    std::fs::write(&payload, "not json")?;
    let request = tmp.join("plan-v1-request.json");
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "write-request-v1"),
            ("VELNOR_REQUEST_FILE", request.to_str().unwrap_or("/")),
            ("GITHUB_EVENT_NAME", "push"),
            ("GITHUB_EVENT_PATH", payload.to_str().unwrap_or("/")),
            ("RUNNER_TEMP", tmp.to_str().unwrap_or("/")),
        ],
        &tmp,
    )?;
    assert_eq!(code(&output), 1);
    assert_eq!(output.stdout, [] as [u8; 0]);
    assert_no_leak(&output);
    assert!(!request.exists());
    cleanup(&tmp);
    Ok(())
}

/// Empty repo with config, committed: plan selects nothing, merge reports no-work.
fn empty_repo() -> Result<std::path::PathBuf, Box<dyn Error>> {
    let tmp = fresh_tempdir("proto-repo")?;
    git_init(&tmp)?;
    assert_eq!(code(&spawn_isolated(&["init"], &[], &tmp)?), 0);
    pin_branch(&tmp)?;
    commit_all(&tmp)?;
    Ok(tmp)
}

/// Stage one event-shape plan request against the repo's real HEAD.
fn stage_plan_request(repo: &Path) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let head = head_sha(repo)?;
    let request = repo.join("plan-v1-request.json");
    std::fs::write(
        &request,
        format!(
            "{{\"schema\":1,\"op\":\"plan-v1\",\"event\":\"push\",\"base\":null,\"head\":\"{head}\",\"root\":\".\"}}"
        ),
    )?;
    Ok(request)
}

#[test]
fn plan_writes_response_and_github_outputs() -> Result<(), Box<dyn Error>> {
    let repo = empty_repo()?;
    let request = stage_plan_request(&repo)?;
    let outputs = repo.join("github-outputs");
    std::fs::write(&outputs, "seed=1\n")?;
    let runner_temp = repo.join("runner-temp");
    std::fs::create_dir_all(&runner_temp)?;
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "plan-v1"),
            ("VELNOR_REQUEST_FILE", request.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ID", "7"),
            ("GITHUB_RUN_ATTEMPT", "2"),
            ("GITHUB_OUTPUT", outputs.to_str().unwrap_or("/")),
            ("RUNNER_TEMP", runner_temp.to_str().unwrap_or("/")),
        ],
        &repo,
    )?;
    assert_eq!(code(&output), 0, "stderr: {:?}", output.stderr);
    assert_eq!(output.stdout, [] as [u8; 0]);
    assert_no_leak(&output);
    let response = std::fs::read_to_string(repo.join("plan-v1-response.json"))?;
    assert!(response.contains("\"run_key\":\"r7-a2\""), "{response}");
    let body = std::fs::read_to_string(&outputs)?;
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 4, "{body}");
    assert_eq!(lines[0], "seed=1");
    let matrix = lines[1].strip_prefix("matrix=").ok_or("matrix line")?;
    let plan = lines[2].strip_prefix("plan=").ok_or("plan line")?;
    assert_eq!(lines[3], "covered_tasks=");
    assert!(matrix.starts_with("{\"include\":"), "{matrix}");
    assert!(plan.contains("\"run_key\":\"r7-a2\""), "{plan}");
    assert!(plan.contains(matrix), "matrix must agree with plan");
    cleanup(&repo);
    Ok(())
}

#[test]
fn merge_no_work_plan_reports_no_work() -> Result<(), Box<dyn Error>> {
    let repo = empty_repo()?;
    let request = stage_plan_request(&repo)?;
    let outputs = repo.join("github-outputs");
    let runner_temp = repo.join("runner-temp");
    std::fs::create_dir_all(&runner_temp)?;
    let plan = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "plan-v1"),
            ("VELNOR_REQUEST_FILE", request.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ID", "7"),
            ("GITHUB_RUN_ATTEMPT", "2"),
            ("GITHUB_OUTPUT", outputs.to_str().unwrap_or("/")),
            ("RUNNER_TEMP", runner_temp.to_str().unwrap_or("/")),
        ],
        &repo,
    )?;
    assert_eq!(code(&plan), 0, "stderr: {:?}", plan.stderr);
    let response = std::fs::read_to_string(repo.join("plan-v1-response.json"))?;
    let rest = response
        .strip_prefix("{\"schema\":1,")
        .ok_or("response shape")?;
    let merge_request = repo.join("merge-v1-request.json");
    std::fs::write(
        &merge_request,
        format!(
            "{{\"schema\":1,\"run_key\":\"r7-a2\",\"actual_event\":\"push\",\"matrix_reports\":[],\"required_job_ids\":[\"plan\"],\"required_jobs\":[{{\"job_id\":\"plan\",\"conclusion\":\"success\"}}],{rest}"
        ),
    )?;
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "merge-v1"),
            ("VELNOR_REQUEST_FILE", merge_request.to_str().unwrap_or("/")),
            ("RUNNER_TEMP", runner_temp.to_str().unwrap_or("/")),
        ],
        &repo,
    )?;
    assert_eq!(code(&output), 0, "stderr: {:?}", output.stderr);
    assert_eq!(output.stdout, [] as [u8; 0]);
    assert_no_leak(&output);
    let verdict = std::fs::read_to_string(repo.join("merge-v1-response.json"))?;
    assert!(verdict.contains("\"status\":\"no_work\""), "{verdict}");
    let published = std::fs::read_to_string(
        runner_temp
            .join("velnor")
            .join("r7-a2")
            .join("final-report.json"),
    )?;
    assert!(published.contains("\"status\":\"no_work\""), "{published}");
    cleanup(&repo);
    Ok(())
}

#[test]
fn plan_publishes_plan_artifact_files() -> Result<(), Box<dyn Error>> {
    let repo = empty_repo()?;
    let request = stage_plan_request(&repo)?;
    let outputs = repo.join("github-outputs");
    let runner_temp = repo.join("runner-temp");
    std::fs::create_dir_all(&runner_temp)?;
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "plan-v1"),
            ("VELNOR_REQUEST_FILE", request.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ID", "7"),
            ("GITHUB_RUN_ATTEMPT", "2"),
            ("GITHUB_OUTPUT", outputs.to_str().unwrap_or("/")),
            ("RUNNER_TEMP", runner_temp.to_str().unwrap_or("/")),
        ],
        &repo,
    )?;
    assert_eq!(code(&output), 0, "stderr: {:?}", output.stderr);
    assert_no_leak(&output);
    let run_dir = runner_temp.join("velnor").join("r7-a2");
    let plan_json = std::fs::read_to_string(run_dir.join("plan.json"))?;
    let matrix_json = std::fs::read_to_string(run_dir.join("matrix.json"))?;
    assert!(plan_json.contains("\"run_key\":\"r7-a2\""), "{plan_json}");
    let body = std::fs::read_to_string(&outputs)?;
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 3, "{body}");
    let matrix = lines[0].strip_prefix("matrix=").ok_or("matrix line")?;
    let plan = lines[1].strip_prefix("plan=").ok_or("plan line")?;
    assert_eq!(lines[2], "covered_tasks=");
    assert_eq!(matrix_json, matrix, "matrix.json agrees with GITHUB_OUTPUT");
    assert_eq!(plan_json, plan, "plan.json agrees with GITHUB_OUTPUT");
    cleanup(&repo);
    Ok(())
}

#[test]
fn plan_without_runner_temp_exits_one() -> Result<(), Box<dyn Error>> {
    let repo = empty_repo()?;
    let request = stage_plan_request(&repo)?;
    let outputs = repo.join("github-outputs");
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "plan-v1"),
            ("VELNOR_REQUEST_FILE", request.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ID", "7"),
            ("GITHUB_RUN_ATTEMPT", "2"),
            ("GITHUB_OUTPUT", outputs.to_str().unwrap_or("/")),
        ],
        &repo,
    )?;
    assert_eq!(code(&output), 1);
    assert_eq!(output.stdout, [] as [u8; 0]);
    assert_no_leak(&output);
    assert!(repo.join("plan-v1-response.json").is_file());
    cleanup(&repo);
    Ok(())
}

#[test]
fn merge_garbage_request_exits_one() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("proto-merge-bad")?;
    let request = tmp.join("merge-v1-request.json");
    std::fs::write(&request, "not json")?;
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "merge-v1"),
            ("VELNOR_REQUEST_FILE", request.to_str().unwrap_or("/")),
        ],
        &tmp,
    )?;
    assert_eq!(code(&output), 1);
    assert_eq!(output.stdout, [] as [u8; 0]);
    assert_no_leak(&output);
    assert!(!tmp.join("merge-v1-response.json").exists());
    cleanup(&tmp);
    Ok(())
}
