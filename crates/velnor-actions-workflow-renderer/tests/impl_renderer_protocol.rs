//! Event-time protocol rendering: write-request/plan/merge env wiring.
use std::collections::BTreeMap;
use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, Trigger, WorkflowIr, WorkflowPolicy,
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
        plan_consumer_env: std::collections::BTreeMap::new(),
        source_helpers: Vec::new(),
        native_pages_approvals: Vec::new(),
        native_publish_approvals: Vec::new(),
    }
}

fn fixture_ir(steps: Vec<velnor_actions_contract::Step>) -> WorkflowIr {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "plan".to_owned(),
        Job {
            cache_mode: None,
            display_name: "Plan".to_owned(),
            runs_on: LABEL.to_owned(),
            timeout_minutes: JobTimeout::PLAN,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            tool_producer: None,
            mbx_producer: None,
            source_producer: None,
            native_pages_deploy: None,
            native_publish: None,
            outputs: Vec::new(),
            environment: None,
            steps,
        },
    );
    WorkflowIr {
        cache_mode: velnor_actions_contract::CacheMode::Read,
        run_name: None,
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: ["opened", "synchronize", "reopened", "ready_for_review"]
                .iter()
                .map(ToString::to_string)
                .collect(),
            push_tags: Vec::new(),
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
fn early_and_ready_promotion_receive_live_auth_only_in_consumer_boundary() -> Result<(), RenderError>
{
    let ir = fixture_ir(vec![
        checkout_step(&checkout_pin())?,
        write_request_step("plan-v1")?,
        velnor_actions_workflow_renderer::early_plan::early_plan_step()?,
        plan_step(),
    ]);
    let mut ctx = fixture_ctx();
    ctx.plan_consumer_env.insert(
        "MISE_DATA_DIR".to_owned(),
        "${{ runner.temp }}/velnor/planning/mise".to_owned(),
    );
    let text = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx)?;
    for channel in [
        "GH_REPO: ${{ github.repository }}",
        "GH_TOKEN: ${{ github.token }}",
    ] {
        assert_eq!(
            text.matches(channel).count(),
            2,
            "early and final Plan: {channel}"
        );
    }
    assert!(text.contains("VELNOR_EARLY_NEEDS_CARGO: ${{ steps.early_plan.outputs.needs_cargo }}"));
    assert_eq!(text.matches("VELNOR_PLAN_FRESHNESS:").count(), 2);
    assert!(
        text.contains("cargo_fallback_required: ${{ steps.plan.outputs.cargo_fallback_required }}")
    );
    assert_eq!(
        text.matches("MISE_DATA_DIR: ${{ runner.temp }}/velnor/planning/mise")
            .count(),
        2,
        "both consumer planning operations share the planning root"
    );
    assert!(!text.contains("MISE_DATA_DIR: ${{ runner.temp }}/velnor/mise"));
    let ordinary = fixture_ir(vec![checkout_step(&checkout_pin())?, plan_step()]);
    let ordinary = render_workflow_ir(&ordinary, WorkflowPolicy::ConsumerV1, None, &fixture_ctx())?;
    assert!(
        !ordinary.contains("GH_TOKEN:"),
        "ordinary plan receives no replay credentials"
    );
    assert_env_gate_only(&text);
    assert!(
        ordinary
            .contains("cargo_fallback_required: ${{ steps.plan.outputs.cargo_fallback_required }}"),
        "ordinary Plan exports its actual publication result"
    );
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
