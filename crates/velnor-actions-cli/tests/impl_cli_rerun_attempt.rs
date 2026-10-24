//! A "re-run failed jobs" attempt judges the plan its first attempt produced.
//!
//! End to end through the binary: `plan-v1` runs in attempt 1 (run key
//! `r7-a1`), then `write-request-v1` and `merge-v1` run in attempt 2 with
//! `VELNOR_RUN_KEY_ATTEMPT=1`, as the generated workflow binds it from the
//! plan job's `plan_attempt` output. The merge must find the attempt-1
//! plan and pass its skipped `not_affected` crate jobs. Without the
//! binding, attempt 2 looks for a plan that never existed and fails closed.

use std::error::Error;
use std::path::Path;

use crate::impl_cli_parity_golden::checkout;
use crate::impl_cli_tmp::{cleanup, code, spawn_isolated};

/// Plan an empty pull-request diff as attempt 1 and stage its artifacts.
fn stage_attempt_one_plan(
    repo: &Path,
    head: &str,
    runner_temp: &Path,
) -> Result<Vec<String>, Box<dyn Error>> {
    let internal = runner_temp.with_file_name("rerun-internal");
    std::fs::create_dir_all(&internal)?;
    let request = internal.join("plan-v1-request.json");
    std::fs::write(
        &request,
        serde_json::json!({
            "schema": 1,
            "run_key": "r7-a1",
            "base": head,
            "head": head,
            "event": "pull_request",
        })
        .to_string(),
    )?;
    let github_output = internal.join("github-output.txt");
    std::fs::write(&github_output, "")?;
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "plan-v1"),
            (
                "VELNOR_REQUEST_FILE",
                request.to_str().ok_or("non-utf8 request")?,
            ),
            ("RUNNER_TEMP", runner_temp.to_str().ok_or("non-utf8 temp")?),
            (
                "GITHUB_OUTPUT",
                github_output.to_str().ok_or("non-utf8 output")?,
            ),
        ],
        repo,
    )?;
    if code(&output) != 0 {
        return Err(format!(
            "plan-v1 failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let response: serde_json::Value =
        serde_json::from_slice(&std::fs::read(internal.join("plan-v1-response.json"))?)?;
    let run_dir = runner_temp.join("velnor/r7-a1");
    std::fs::create_dir_all(&run_dir)?;
    for (name, value) in [
        ("plan.json", &response["plan"]),
        ("matrix.json", &response["matrix"]),
    ] {
        if !run_dir.join(name).exists() {
            std::fs::write(run_dir.join(name), value.to_string())?;
        }
    }
    let skipped = response["plan"]["jobs"]
        .as_array()
        .ok_or("plan lists jobs")?
        .iter()
        .filter(|job| job["disposition"] == "not_affected")
        .filter_map(|job| job["job_id"].as_str().map(str::to_owned))
        .collect();
    Ok(skipped)
}

/// Run `write-request-v1` then `merge-v1` as attempt 2; return the status.
fn merge_attempt_two(
    repo: &Path,
    runner_temp: &Path,
    skipped: &[String],
    plan_attempt: Option<&str>,
    name: &str,
) -> Result<String, Box<dyn Error>> {
    let mut needs = serde_json::Map::new();
    needs.insert("plan".to_owned(), serde_json::json!({"result": "success"}));
    for id in skipped {
        needs.insert(id.clone(), serde_json::json!({"result": "skipped"}));
    }
    let expected: Vec<&String> = needs.keys().collect();
    let needs_json = serde_json::Value::Object(needs.clone()).to_string();
    let expected_json = serde_json::to_string(&expected)?;
    let payload = runner_temp.with_file_name(format!("{name}-event.json"));
    std::fs::write(
        &payload,
        r#"{"pull_request":{"head":{"repo":{"fork":false}}}}"#,
    )?;
    let request = runner_temp.join(format!("request-{name}/merge-v1-request.json"));
    let temp = runner_temp.to_str().ok_or("non-utf8 temp")?;
    let request_text = request.to_str().ok_or("non-utf8 request")?;
    let payload_text = payload.to_str().ok_or("non-utf8 payload")?;
    let mut env = vec![
        ("RUNNER_TEMP", temp),
        ("GITHUB_RUN_ID", "7"),
        ("GITHUB_RUN_ATTEMPT", "2"),
        ("GITHUB_EVENT_NAME", "pull_request"),
        ("GITHUB_EVENT_PATH", payload_text),
        ("VELNOR_NEEDS_JSON", needs_json.as_str()),
        ("VELNOR_NEEDS_EXPECTED", expected_json.as_str()),
        ("VELNOR_REQUEST_FILE", request_text),
    ];
    // An empty value is "unset" to the resolver; set it explicitly so a CI
    // runner's own VELNOR_RUN_KEY_ATTEMPT never leaks into the child.
    env.push(("VELNOR_RUN_KEY_ATTEMPT", plan_attempt.unwrap_or("")));
    for op in ["write-request-v1", "merge-v1"] {
        let mut vars = env.clone();
        vars.push(("VELNOR_INTERNAL_OP", op));
        let output = spawn_isolated(&[], &vars, repo)?;
        if op == "write-request-v1" && code(&output) != 0 {
            return Err(format!("{op} failed: {}", String::from_utf8_lossy(&output.stderr)).into());
        }
    }
    let response = request.with_file_name("merge-v1-response.json");
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(response)?)?;
    if report["status"] != "passed" {
        eprintln!("{name} merge report: {report}");
    }
    Ok(report["status"].as_str().unwrap_or_default().to_owned())
}

#[test]
fn attempt_two_merges_the_plan_attempt_one_produced() -> Result<(), Box<dyn Error>> {
    let (repo, head) = checkout("minimal-cargo")?;
    let runner_temp = repo.with_file_name("runner-temp");
    std::fs::create_dir_all(&runner_temp)?;
    let skipped = stage_attempt_one_plan(&repo, &head, &runner_temp)?;
    assert!(!skipped.is_empty(), "an empty diff skips the crate job");
    let bound = merge_attempt_two(&repo, &runner_temp, &skipped, Some("1"), "bound")?;
    assert_eq!(bound, "passed", "attempt 2 judges the attempt-1 plan");
    let unbound = merge_attempt_two(&repo, &runner_temp, &skipped, None, "unbound")?;
    assert_ne!(
        unbound, "passed",
        "attempt 2 without the binding finds no plan"
    );
    if let Some(parent) = repo.parent() {
        cleanup(parent);
    }
    Ok(())
}
