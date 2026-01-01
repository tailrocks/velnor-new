//! Event-time protocol: write-request materialization, plan outputs, merge verdict.

use std::error::Error;
use std::path::Path;

use crate::impl_cli_tmp::{
    cleanup, code, commit_all, fresh_tempdir, git_init, head_sha, install_consumer_manifest,
    spawn_isolated, write_workspace,
};

/// Serialized workflow marker selecting dynamic matrix output accounting.
const PLAN_MATRIX_OUTPUT_MODE_ENV: &str = "VELNOR_PLAN_MATRIX_OUTPUT_MODE";
/// Exact serialized marker value emitted by the renderer.
const DYNAMIC_MATRIX_OUTPUT_MODE: &str = "dynamic_matrix";

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
    assert!(output.stdout.is_empty());
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
    assert!(output.stdout.is_empty());
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
    install_consumer_manifest(&tmp)?;
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
            (PLAN_MATRIX_OUTPUT_MODE_ENV, DYNAMIC_MATRIX_OUTPUT_MODE),
        ],
        &repo,
    )?;
    assert_eq!(code(&output), 0, "stderr: {:?}", output.stderr);
    assert!(output.stdout.is_empty());
    assert_no_leak(&output);
    let response = std::fs::read_to_string(repo.join("plan-v1-response.json"))?;
    assert!(response.contains("\"run_key\":\"r7-a2\""), "{response}");
    let body = std::fs::read_to_string(&outputs)?;
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 10, "{body}");
    assert_eq!(lines[0], "seed=1");
    let matrix = lines[1].strip_prefix("matrix=").ok_or("matrix line")?;
    assert_eq!(lines[2], "plan_id=plan-r7-a2");
    assert_eq!(lines[3], "run_key=r7-a2");
    assert_eq!(lines[4], "covered_tasks=");
    assert_eq!(lines[5], "qualification_campaign=");
    assert_eq!(lines[6], "qualification_phase=");
    assert_eq!(lines[7], "qualification_cache_enabled=false");
    assert_eq!(lines[8], "qualification_cache_write=false");
    assert_eq!(lines[9], "qualification_cache_directives=");
    assert!(matrix.starts_with("{\"include\":"), "{matrix}");
    let response: serde_json::Value = serde_json::from_str(&response)?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(matrix)?,
        response["matrix"]
    );
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
    assert!(output.stdout.is_empty());
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
    assert_eq!(lines.len(), 9, "{body}");
    let matrix = lines[0].strip_prefix("matrix=").ok_or("matrix line")?;
    assert_eq!(lines[1], "plan_id=plan-r7-a2");
    assert_eq!(lines[2], "run_key=r7-a2");
    assert_eq!(lines[3], "covered_tasks=");
    assert_eq!(lines[4], "qualification_campaign=");
    assert_eq!(lines[5], "qualification_phase=");
    assert_eq!(lines[6], "qualification_cache_enabled=false");
    assert_eq!(lines[7], "qualification_cache_write=false");
    assert_eq!(lines[8], "qualification_cache_directives=");
    assert_eq!(matrix_json, matrix, "matrix.json agrees with GITHUB_OUTPUT");
    let plan: serde_json::Value = serde_json::from_str(&plan_json)?;
    assert_eq!(
        plan["matrix"],
        serde_json::from_str::<serde_json::Value>(matrix)?
    );
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
    assert!(output.stdout.is_empty());
    assert_no_leak(&output);
    assert!(repo.join("plan-v1-response.json").is_file());
    cleanup(&repo);
    Ok(())
}

#[test]
fn plan_rejects_unknown_output_mode_before_computation() -> Result<(), Box<dyn Error>> {
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
            ("GITHUB_OUTPUT", outputs.to_str().unwrap_or("/")),
            ("RUNNER_TEMP", runner_temp.to_str().unwrap_or("/")),
            (PLAN_MATRIX_OUTPUT_MODE_ENV, "unknown"),
        ],
        &repo,
    )?;
    assert_eq!(code(&output), 1);
    assert!(String::from_utf8_lossy(&output.stderr).contains("bad_plan_matrix_output_mode"));
    assert_no_leak(&output);
    assert!(!repo.join("plan-v1-response.json").exists());
    assert_eq!(std::fs::read_to_string(&outputs)?, "seed=1\n");
    assert!(!runner_temp.join("velnor").exists());
    cleanup(&repo);
    Ok(())
}

#[test]
fn dynamic_matrix_over_job_limit_preserves_response_but_publishes_nothing()
-> Result<(), Box<dyn Error>> {
    let repo = empty_repo()?;
    write_workspace(&repo, 65)?;
    commit_all(&repo)?;
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
            (PLAN_MATRIX_OUTPUT_MODE_ENV, DYNAMIC_MATRIX_OUTPUT_MODE),
        ],
        &repo,
    )?;
    assert_eq!(code(&output), 1, "stderr: {:?}", output.stderr);
    assert!(String::from_utf8_lossy(&output.stderr).contains("matrix_jobs_exceeded"));
    assert_no_leak(&output);
    let response = std::fs::read_to_string(repo.join("plan-v1-response.json"))?;
    let response: serde_json::Value = serde_json::from_str(&response)?;
    assert_eq!(response["schema"], 1);
    assert!(
        response["matrix"]["include"]
            .as_array()
            .is_some_and(|rows| rows.len() > 256)
    );
    assert_eq!(std::fs::read_to_string(&outputs)?, "seed=1\n");
    assert!(!runner_temp.join("velnor").exists());
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
    assert!(output.stdout.is_empty());
    assert_no_leak(&output);
    assert!(!tmp.join("merge-v1-response.json").exists());
    cleanup(&tmp);
    Ok(())
}
