//! Event-time protocol: render-gate roundtrip for the plan job.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, Trigger, WorkflowIr, WorkflowPolicy,
};
use velnor_actions_orchestrator::write_request_parts;
use velnor_actions_workflow_renderer::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, checkout_step, plan_step, render::RenderContext,
    render_workflow_ir, steps::write_request_step,
};

use crate::impl_common::TestResult;

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
        "plan".to_owned(),
        Job {
            check_runner: None,
            display_name: "Plan".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            timeout_minutes: JobTimeout::PLAN,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
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
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
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
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        verification_tasks: Vec::new(),
        plan_consumer_env: std::collections::BTreeMap::new(),
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
    write_request_parts(
        Path::new(&file),
        "push",
        &payload,
        Some(&head),
        None,
        dir.path(),
    )?;
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file)?)?;
    assert_eq!(value["op"], "plan-v1");
    assert_eq!(value["event"], "push");
    assert_eq!(value["head"], head);
    Ok(())
}
