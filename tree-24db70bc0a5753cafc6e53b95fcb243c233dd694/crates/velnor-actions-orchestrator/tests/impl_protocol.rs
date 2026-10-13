//! Event-time protocol: request materialization, outputs, verdict, render gate.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::{
    Concurrency, Job, Permissions, Trigger, WorkflowIr, WorkflowPolicy, canonical_json_str,
};
use velnor_actions_orchestrator::{
    assemble_merge_request, merge_internal, merge_passed, plan_internal, plan_outputs,
    publish_plan_files, response_path_for, write_request_parts,
};
use velnor_actions_workflow_renderer::render::RenderContext;
use velnor_actions_workflow_renderer::steps::write_request_step;
use velnor_actions_workflow_renderer::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, checkout_step, plan_step, render_workflow_ir,
};

use crate::impl_common::{
    TestResult, config_with_branch, err_of, git, git_line, make_repo, passing_reports,
    plan_for_source_change,
};

#[test]
fn write_request_materializes_pull_request() -> TestResult {
    let dir = TempDir::new()?;
    let base = "b".repeat(64);
    let head = "a".repeat(64);
    let payload = format!(
        "{{\"pull_request\":{{\"base\":{{\"sha\":\"{base}\"}},\"head\":{{\"sha\":\"{head}\"}}}}}}"
    );
    let file = dir
        .path()
        .join("velnor")
        .join("request")
        .join("plan-v1-request.json");
    let written = write_request_parts(&file, "pull_request", &payload, None)?;
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
fn write_request_materializes_push_without_base() -> TestResult {
    let dir = TempDir::new()?;
    let head = "c".repeat(40);
    let payload = format!("{{\"before\":\"{}\",\"after\":\"{head}\"}}", "0".repeat(40));
    let file = dir.path().join("plan-v1-request.json");
    write_request_parts(&file, "push", &payload, Some(&head))?;
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
    write_request_parts(&fallback, "push", &nulls, Some(&sha))?;
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
    write_request_parts(&file, "merge_group", &payload, None)?;
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
        write_request_parts(&bad_op, "push", payload, None),
        "unknown op refused",
    )?;
    assert!(err.to_string().contains("unknown_request_op"), "{err}");
    assert!(!bad_op.exists());
    let bad_event = dir.path().join("plan-v1-request.json");
    let err = err_of(
        write_request_parts(&bad_event, "schedule", payload, None),
        "unknown event refused",
    )?;
    assert!(err.to_string().contains("unsupported_event"), "{err}");
    let err = err_of(
        write_request_parts(&bad_event, "push", "not json", None),
        "malformed payload refused",
    )?;
    assert!(err.to_string().contains("malformed_event_payload"), "{err}");

    fs::write(&bad_event, "{}")?;
    let err = err_of(
        write_request_parts(&bad_event, "push", payload, None),
        "existing file refused",
    )?;
    assert!(err.to_string().contains("request_exists"), "{err}");
    assert_eq!(fs::read_to_string(&bad_event)?, "{}");
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
    let outputs = plan_outputs(&response)?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    assert_eq!(outputs.plan, canonical_json_str(&value["plan"])?);
    assert_eq!(outputs.matrix, canonical_json_str(&value["matrix"])?);
    assert!(!outputs.matrix.contains('\n'));
    assert!(outputs.plan.contains(&outputs.matrix));
    assert!(err_of(plan_outputs("not json"), "outputs reject garbage").is_ok());
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
    for (index, report) in reports.iter().enumerate() {
        fs::write(
            run.join("reports").join(format!("r{index}.json")),
            serde_json::to_string(report)?,
        )?;
    }
    let request = assemble_merge_request("local", &run)?;
    let value: serde_json::Value = serde_json::from_str(&request)?;
    assert!(value.get("base").is_none(), "merge shape: {request}");
    let final_report: velnor_actions_contract::FinalReport =
        serde_json::from_str(&merge_internal(&request)?)?;
    assert_eq!(
        final_report.status,
        velnor_actions_contract::FinalStatus::Passed
    );
    Ok(())
}

#[test]
fn merge_assembly_rejects_missing_plan() -> TestResult {
    let dir = TempDir::new()?;
    let err = err_of(
        assemble_merge_request("local", dir.path()),
        "missing plan refused",
    )?;
    assert!(err.to_string().contains("missing_plan_artifact"), "{err}");
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
        ("not_run", false),
        ("planning_failed", false),
    ] {
        let response = format!(r#"{{"schema":1,"status":"{status}"}}"#);
        assert_eq!(merge_passed(&response)?, passed, "{status}");
    }
    assert!(err_of(merge_passed("not json"), "verdict rejects garbage").is_ok());
    Ok(())
}

/// Consecutive `VELNOR_INTERNAL_OP`/`VELNOR_REQUEST_FILE` YAML pairs.
fn internal_env_pairs(text: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut op: Option<&str> = None;
    for line in text.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("VELNOR_INTERNAL_OP: ") {
            op = Some(value.trim_matches('"'));
        } else if let Some(value) = line.strip_prefix("VELNOR_REQUEST_FILE: ")
            && let Some(first) = op.take()
        {
            pairs.push((first.to_owned(), value.trim_matches('"').to_owned()));
        }
    }
    pairs
}

/// Render the plan job, extract its gate env, run the gate logic, accept.
#[test]
fn render_gate_roundtrip_accepts_plan() -> TestResult {
    let pin = format!("actions/checkout@{:040x}", 0);
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "velnor-plan".to_owned(),
        Job {
            display_name: "Velnor Plan".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            needs: Vec::new(),
            condition: None,
            steps: vec![
                checkout_step(&pin)?,
                write_request_step("plan-v1")?,
                plan_step(),
            ],
        },
    );
    let ir = WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: ["opened", "synchronize", "reopened", "ready_for_review"]
                .iter()
                .map(ToString::to_string)
                .collect(),
            push_branches: vec!["main".to_owned()],
            merge_group: true,
        },
        permissions: Permissions {
            contents: "read".to_owned(),
            actions: "read".to_owned(),
        },
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs,
    };
    let ctx = RenderContext {
        generator_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: pin,
        policy_commands: Vec::new(),
        candidate: None,
        preseed: false,
    };
    let text = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx)?;
    let pairs = internal_env_pairs(&text);
    assert_eq!(pairs.len(), 2, "{text}");
    let request_template = "${{ runner.temp }}/velnor/request/plan-v1-request.json";
    assert!(pairs.contains(&("write-request-v1".to_owned(), request_template.to_owned())));
    assert!(pairs.contains(&("plan-v1".to_owned(), request_template.to_owned())));

    let dir = TempDir::new()?;
    let head = "f".repeat(40);
    let payload = format!("{{\"before\":null,\"after\":\"{head}\"}}");
    let file = request_template.replace("${{ runner.temp }}", &dir.path().display().to_string());
    write_request_parts(Path::new(&file), "push", &payload, Some(&head))?;
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file)?)?;
    assert_eq!(value["op"], "plan-v1");
    assert_eq!(value["event"], "push");
    assert_eq!(value["head"], head);
    Ok(())
}
