//! Event-time protocol rendering: write-request/plan/merge env wiring.
use std::collections::BTreeMap;
use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_contract_workflow::{
    Concurrency, Job, JobTimeout, Permissions, Trigger, WorkflowIr,
};
use velnor_actions_workflow_renderer::steps::{
    WRITE_REQUEST_OPERATION, download_artifact_step, write_request_step,
};
use velnor_actions_workflow_renderer::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, FORBIDDEN_TOKENS, INTERNAL_OP_ENV, REQUEST_FILE_ENV,
    RenderContext, RenderError, checkout_step, internal_step, merge_step, plan_step,
    render_workflow_ir,
};

const VERSION: &str = "0.1.0";
const LABEL: &str = "ubuntu-26.04";
const REQUEST_DIR: &str = "${{ runner.temp }}/velnor/request";

fn checkout_pin() -> String {
    format!("actions/checkout@{:040x}", 0)
}

fn fixture_ctx() -> RenderContext {
    RenderContext {
        generator_version: VERSION.to_owned(),
        runs_on: LABEL.to_owned(),
        staged_binary: format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{VERSION}"),
        request_dir: REQUEST_DIR.to_owned(),
        checkout_uses: checkout_pin(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        verification_tasks: Vec::new(),
        plan_consumer_env: std::collections::BTreeMap::new(),
    }
}

fn fixture_ir(steps: Vec<velnor_actions_contract_workflow::Step>) -> WorkflowIr {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "plan".to_owned(),
        Job {
            display_name: "Plan".to_owned(),
            runs_on: LABEL.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::PLAN,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps,
        },
    );
    WorkflowIr {
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
    }
}

/// Every `run:` line must carry the binary only, never an op tag.
fn assert_env_gate_only(text: &str) {
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("run:"))
    {
        assert!(!line.contains("plan-v1"), "argv leak: {line}");
        assert!(!line.contains("merge-v1"), "argv leak: {line}");
        assert!(!line.contains("write-request-v1"), "argv leak: {line}");
    }
    for token in FORBIDDEN_TOKENS {
        assert!(!text.contains(token), "leaked token: {token}");
    }
}

#[test]
fn plan_sequence_wires_write_request_before_plan() -> Result<(), RenderError> {
    let ir = fixture_ir(vec![
        checkout_step(&checkout_pin())?,
        write_request_step("plan-v1")?,
        plan_step(),
    ]);
    let text = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &fixture_ctx())?;
    assert!(text.contains(&format!("{INTERNAL_OP_ENV}: {WRITE_REQUEST_OPERATION}")));
    assert!(text.contains(&format!(
        "{REQUEST_FILE_ENV}: {REQUEST_DIR}/plan-v1-request.json"
    )));
    assert!(text.contains(&format!("{INTERNAL_OP_ENV}: plan-v1")));
    assert_env_gate_only(&text);
    Ok(())
}

#[test]
fn merge_sequence_targets_merge_request() -> Result<(), RenderError> {
    let ir = fixture_ir(vec![
        checkout_step(&checkout_pin())?,
        write_request_step("merge-v1")?,
        merge_step(),
    ]);
    let text = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &fixture_ctx())?;
    assert!(text.contains(&format!("{INTERNAL_OP_ENV}: {WRITE_REQUEST_OPERATION}")));
    assert!(text.contains(&format!(
        "{REQUEST_FILE_ENV}: {REQUEST_DIR}/merge-v1-request.json"
    )));
    assert!(text.contains(&format!("{INTERNAL_OP_ENV}: merge-v1")));
    assert_env_gate_only(&text);
    Ok(())
}

#[test]
fn write_request_targets_known_ops_only() {
    assert!(write_request_step("plan-v1").is_ok());
    assert!(write_request_step("merge-v1").is_ok());
    assert!(write_request_step("bogus-v9").is_err());
    assert!(internal_step("Write request", "write-request-v1:plan-v1").is_ok());
    assert!(internal_step("Write request", "write-request-v1").is_err());
    assert!(internal_step("Write request", "write-request-v1:bogus-v9").is_err());
    assert!(internal_step("Plan", "bogus-v9").is_err());
}

#[test]
fn download_reports_use_explicit_names() -> Result<(), RenderError> {
    let step = download_artifact_step("velnor-plan-response", "${{ runner.temp }}/velnor/request")?;
    let ir = fixture_ir(vec![checkout_step(&checkout_pin())?, step]);
    let text = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &fixture_ctx())?;
    assert!(text.contains("name: velnor-plan-response"));
    assert!(!text.contains('*'), "wildcard download forbidden");
    Ok(())
}
