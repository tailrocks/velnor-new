//! Event-time protocol: request materialization, outputs, verdict.

use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::canonical_json_str;
use velnor_actions_orchestrator::{
    PlanOutputMode, assemble_merge_request, merge_internal, merge_passed, plan_internal,
    plan_outputs, publish_plan_files, response_path_for, write_request_parts,
};

use crate::impl_common::{
    TestResult, config_with_branch, err_of, git, git_line, make_repo, passing_reports,
    plan_for_source_change,
};
use crate::impl_merge::task_reports_for;
use crate::impl_orch_core_cover::covered_plan;

#[test]
fn write_request_materializes_pull_request() -> TestResult {
    let dir = TempDir::new()?;
    let base = "b".repeat(64);
    let head = "a".repeat(64);
    let payload = format!(
        "{{\"pull_request\":{{\"base\":{{\"sha\":\"{base}\"}},\"head\":{{\"sha\":\"{head}\",\"repo\":{{\"fork\":false}}}}}}}}"
    );
    let file = dir
        .path()
        .join("velnor")
        .join("request")
        .join("plan-v1-request.json");
    let written = write_request_parts(&file, "pull_request", &payload, None, None, dir.path())?;
    assert_eq!(written, file);
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file)?)?;
    assert_eq!(value["schema"], 1);
    assert_eq!(value["op"], "plan-v1");
    assert_eq!(value["event"], "pull_request");
    assert_eq!(value["base"], base);
    assert_eq!(value["head"], head);
    assert_eq!(value["root"], ".");
    let keys: Vec<&str> = value
        .as_object()
        .map(|map| map.keys().map(String::as_str).collect())
        .unwrap_or_default();
    assert_eq!(keys, ["base", "event", "head", "op", "root", "schema"]);
    Ok(())
}

#[test]
fn write_request_captures_repository_capability() -> TestResult {
    let dir = TempDir::new()?;
    let head = "a".repeat(40);
    let payload = format!("{{\"before\":null,\"after\":\"{head}\"}}");
    let file = dir.path().join("plan-v1-request.json");
    write_request_parts(&file, "push", &payload, None, Some("o/r"), dir.path())?;
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file)?)?;
    assert_eq!(value["repository"], "o/r");
    let bare = dir.path().join("bare").join("plan-v1-request.json");
    write_request_parts(&bare, "push", &payload, None, None, dir.path())?;
    let bare_value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&bare)?)?;
    assert!(
        bare_value.get("repository").is_none(),
        "unset slug stays omitted: {bare_value}"
    );
    Ok(())
}

#[test]
fn write_request_materializes_push_without_base() -> TestResult {
    let dir = TempDir::new()?;
    let head = "c".repeat(40);
    let payload = format!("{{\"before\":\"{}\",\"after\":\"{head}\"}}", "0".repeat(40));
    let file = dir.path().join("plan-v1-request.json");
    write_request_parts(&file, "push", &payload, Some(&head), None, dir.path())?;
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file)?)?;
    assert_eq!(value["event"], "push");
    assert!(value["base"].is_null(), "zero before maps to null");
    assert_eq!(value["head"], head);

    let sha = "d".repeat(40);
    let fallback = dir.path().join("sub").join("plan-v1-request.json");
    let nulls = format!(
        "{{\"before\":\"{}\",\"after\":\"{}\"}}",
        "0".repeat(40),
        "0".repeat(40)
    );
    write_request_parts(&fallback, "push", &nulls, Some(&sha), None, dir.path())?;
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&fallback)?)?;
    assert_eq!(value["head"], sha);
    Ok(())
}

#[test]
fn write_request_materializes_merge_group() -> TestResult {
    let dir = TempDir::new()?;
    let base = "d".repeat(40);
    let head = "e".repeat(40);
    let payload =
        format!("{{\"merge_group\":{{\"base_sha\":\"{base}\",\"head_sha\":\"{head}\"}}}}");
    let file = dir.path().join("plan-v1-request.json");
    write_request_parts(&file, "merge_group", &payload, None, None, dir.path())?;
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file)?)?;
    assert_eq!(value["op"], "plan-v1");
    assert_eq!(value["event"], "merge_group");
    assert_eq!(value["base"], base);
    assert_eq!(value["head"], head);
    Ok(())
}

#[test]
fn write_request_rejects_bad_inputs() -> TestResult {
    let dir = TempDir::new()?;
    let payload = r#"{"before":"abc","after":"def"}"#;
    let bad_op = dir.path().join("bogus-v9-request.json");
    let err = err_of(
        write_request_parts(&bad_op, "push", payload, None, None, dir.path()),
        "unknown op refused",
    )?;
    assert!(err.to_string().contains("unknown_request_op"), "{err}");
    assert!(!bad_op.exists());
    let bad_event = dir.path().join("plan-v1-request.json");
    let err = err_of(
        write_request_parts(&bad_event, "schedule", payload, None, None, dir.path()),
        "unknown event refused",
    )?;
    assert!(err.to_string().contains("unsupported_event"), "{err}");
    let err = err_of(
        write_request_parts(&bad_event, "push", "not json", None, None, dir.path()),
        "malformed payload refused",
    )?;
    assert!(err.to_string().contains("malformed_event_payload"), "{err}");

    fs::write(&bad_event, "{}")?;
    let err = err_of(
        write_request_parts(&bad_event, "push", payload, None, None, dir.path()),
        "existing file refused",
    )?;
    assert!(err.to_string().contains("request_exists"), "{err}");
    assert_eq!(fs::read_to_string(&bad_event)?, "{}");
    Ok(())
}

#[test]
fn write_request_pins_parents_to_the_anchor() -> TestResult {
    let dir = TempDir::new()?;
    let payload = r#"{"before":"abc","after":"def"}"#;
    let elsewhere = TempDir::new()?;
    let escaped = elsewhere.path().join("plan-v1-request.json");
    let err = err_of(
        write_request_parts(&escaped, "push", payload, None, None, dir.path()),
        "anchor escape refused",
    )?;
    assert!(err.to_string().contains("anchor_escape"), "{err}");
    assert!(!escaped.exists());
    #[cfg(unix)]
    {
        let real = dir.path().join("real");
        fs::create_dir(&real)?;
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link)?;
        let planted = link.join("plan-v1-request.json");
        let err = err_of(
            write_request_parts(&planted, "push", payload, None, None, dir.path()),
            "linked parent refused",
        )?;
        assert!(err.to_string().contains("symlink_refused"), "{err}");
        assert!(!real.join("plan-v1-request.json").exists());
    }
    Ok(())
}

#[test]
fn response_sibling_derivation() -> TestResult {
    let dir = TempDir::new()?;
    for op in ["plan-v1", "merge-v1"] {
        let request = dir.path().join(format!("{op}-request.json"));
        let sibling = response_path_for(&request)?;
        assert_eq!(sibling, dir.path().join(format!("{op}-response.json")));
    }
    assert!(response_path_for(&dir.path().join("plan-v1.json")).is_err());
    assert!(response_path_for(&dir.path().join("-request.json")).is_err());
    Ok(())
}

#[test]
fn plan_outputs_agree_with_plan_matrix() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": null,
        "head": head,
        "event": "push",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let outputs = plan_outputs(&response, PlanOutputMode::Static)?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    assert_eq!(outputs.matrix, canonical_json_str(&value["matrix"])?);
    assert!(!outputs.matrix.contains('\n'));
    assert!(
        outputs.covered_tasks.is_empty(),
        "execute-all plans emit no channel"
    );
    assert!(
        err_of(
            plan_outputs("not json", PlanOutputMode::Static),
            "outputs reject garbage"
        )
        .is_ok()
    );
    Ok(())
}

#[test]
fn plan_outputs_encode_covered_tasks() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    assert!(!plan.task_ids.is_empty(), "fixture must select work");
    let (plan_json, _) = covered_plan(&plan)?;
    let response = serde_json::json!({
        "schema": 1,
        "plan": plan_json,
        "matrix": plan_json["matrix"],
    });
    let outputs = plan_outputs(&response.to_string(), PlanOutputMode::Static)?;
    let mut ids: Vec<&str> = plan
        .obligations
        .iter()
        .map(|obligation| obligation.task_id.as_str())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(outputs.covered_tasks, format!(",{},", ids.join(",")));
    Ok(())
}

/// Producer/consumer agreement: assembled files feed the merge unchanged.
#[test]
fn merge_assembled_request_roundtrips_to_passed() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    assert!(!reports.is_empty(), "fixture must carry reports");
    let dir = TempDir::new()?;
    let run = dir.path().join("run");
    fs::create_dir_all(run.join("reports"))?;
    fs::write(run.join("plan.json"), serde_json::to_string(&plan)?)?;
    fs::write(
        run.join("matrix.json"),
        serde_json::to_string(&plan.matrix)?,
    )?;
    let plan_value = serde_json::to_value(&plan)?;
    let task_files = task_reports_for(&plan_value, &serde_json::to_value(&reports)?);
    for report in &reports {
        let entry = plan
            .matrix
            .include
            .iter()
            .find(|entry| entry.report_id == report.report_id)
            .ok_or_else(|| std::io::Error::other("report without entry"))?;
        let dir = run
            .join("reports")
            .join(&entry.artifact_id)
            .join(&entry.matrix_key);
        fs::create_dir_all(dir.join("tasks"))?;
        fs::write(
            dir.join("matrix-report.json"),
            serde_json::to_string(report)?,
        )?;
        for task in &report.tasks {
            let want = Some(task.task_report_id.as_str());
            let file = task_files
                .as_array()
                .and_then(|files| files.iter().find(|f| f["task_report_id"].as_str() == want))
                .ok_or_else(|| std::io::Error::other("task without file"))?;
            let path = dir
                .join("tasks")
                .join(format!("{}.json", task.task_report_id));
            fs::write(path, serde_json::to_string(file)?)?;
        }
    }
    let request = assemble_merge_request("local", &run)?;
    let mut value: serde_json::Value = serde_json::from_str(&request)?;
    assert!(value.get("base").is_none(), "merge shape: {request}");
    // The needs channel is env-provided (unit-tested); patch it in to prove
    // artifact agreement end to end without racing process-global env.
    value["required_job_ids"] = serde_json::json!(["plan"]);
    value["required_jobs"] = serde_json::json!([{"job_id": "plan", "conclusion": "success"}]);
    value["assembly_errors"] = serde_json::json!([]);
    value["actual_event"] = value["plan"]["event"].clone();
    let final_report: velnor_actions_contract_workflow::FinalReport =
        serde_json::from_str(&merge_internal(&value.to_string())?)?;
    assert_eq!(
        final_report.status,
        velnor_actions_contract_workflow::FinalStatus::Passed
    );
    Ok(())
}

#[test]
fn merge_assembly_nulls_missing_plan_to_planning_failed() -> TestResult {
    let dir = TempDir::new()?;
    let request = assemble_merge_request("local", dir.path())?;
    let value: serde_json::Value = serde_json::from_str(&request)?;
    assert!(value["plan"].is_null(), "null plan: {request}");
    assert!(
        value["assembly_errors"]
            .as_array()
            .is_some_and(|e| e.len() >= 3),
        "gaps recorded: {request}"
    );
    let final_report: velnor_actions_contract_workflow::FinalReport =
        serde_json::from_str(&merge_internal(&request)?)?;
    final_report.validate()?;
    assert_eq!(
        final_report.status,
        velnor_actions_contract_workflow::FinalStatus::PlanningFailed
    );
    for token in ["source_missing", "no_entry"] {
        assert!(
            final_report.miss_reasons.contains(&token.to_owned()),
            "diagnosed: {:?}",
            final_report.miss_reasons
        );
    }
    Ok(())
}

/// Plan artifact: response publishes the exact pair `Publish plan` uploads.
#[test]
fn publish_plan_files_writes_artifact_pair() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": null,
        "head": head,
        "event": "push",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let dir = TempDir::new()?;
    let run = publish_plan_files(&response, &dir.path().join("velnor"))?;
    assert_eq!(run, dir.path().join("velnor").join("local"));
    let plan_text = fs::read_to_string(run.join("plan.json"))?;
    let matrix_text = fs::read_to_string(run.join("matrix.json"))?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    assert_eq!(plan_text, canonical_json_str(&value["plan"])?);
    assert_eq!(matrix_text, canonical_json_str(&value["matrix"])?);
    assert!(matrix_text.starts_with("{\"include\":"), "{matrix_text}");
    let err = err_of(
        publish_plan_files(&response, &dir.path().join("velnor")),
        "second publish refused",
    )?;
    assert!(err.to_string().contains("plan_artifact_exists"), "{err}");
    assert!(
        err_of(
            publish_plan_files("not json", dir.path()),
            "garbage refused"
        )
        .is_ok()
    );
    Ok(())
}

#[test]
fn merge_verdict_mapping() -> TestResult {
    for (status, passed) in [
        ("passed", true),
        ("no_work", true),
        ("failed", false),
        ("cancelled", false),
        ("blocked", false),
        ("not_run", false),
        ("planning_failed", false),
    ] {
        let response = format!(r#"{{"schema":1,"status":"{status}"}}"#);
        assert_eq!(merge_passed(&response)?, passed, "{status}");
    }
    assert!(err_of(merge_passed("not json"), "verdict rejects garbage").is_ok());
    Ok(())
}
