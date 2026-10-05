//! Workflow/tree invariant cases (triggers, concurrency, candidate, gates).
use std::collections::BTreeMap;
use velnor_actions_contract::{
    Concurrency, GeneratorValidation, Job, JobTimeout, Permissions, Step, Trigger, ValidatorKind,
    WorkflowIr, WorkflowPolicy,
};
use velnor_actions_workflow_renderer::{
    CANDIDATE_JOB_ID, CONCURRENCY_CANCEL, CONCURRENCY_GROUP, CandidateSpec, RenderContext,
    RenderError, ValidatorCommand, checkout_step, merge_step, plan_step, render_workflow_ir,
    with_marker,
};

#[path = "impl_renderer_tree_policy_candidates.rs"]
mod candidates;

const VERSION: &str = "0.1.0";
const LABEL: &str = "ubuntu-26.04";

fn checkout_pin() -> String {
    format!("actions/checkout@{:040x}", 0)
}

pub(crate) fn fixture_ctx() -> RenderContext {
    RenderContext {
        generator_version: VERSION.to_owned(),
        runs_on: LABEL.to_owned(),
        staged_binary: format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{VERSION}"),
        request_dir: "${{ runner.temp }}/velnor/r1-a1".to_owned(),
        checkout_uses: checkout_pin(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        verification_tasks: Vec::new(),
        plan_consumer_env: std::collections::BTreeMap::new(),
    }
}

fn plan_job() -> Result<Job, RenderError> {
    Ok(Job {
        display_name: "Plan".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![checkout_step(&checkout_pin())?, plan_step()],
    })
}

pub(crate) fn validator_commands() -> Vec<ValidatorCommand> {
    // Production names: the scrub gate allowlists these exactly.
    [
        (ValidatorKind::CargoDeny, "Run cargo-deny"),
        (ValidatorKind::CargoMachete, "Run cargo-machete"),
        (ValidatorKind::Zizmor, "Run zizmor"),
    ]
    .iter()
    .map(|(validator, name)| ValidatorCommand {
        validator: *validator,
        name: (*name).to_owned(),
        argv: vec!["deny".to_owned()],
        prepare_argv: Vec::new(),
    })
    .collect()
}

pub(crate) fn fixture_ir() -> Result<WorkflowIr, RenderError> {
    let mut jobs = BTreeMap::new();
    jobs.insert("plan".to_owned(), plan_job()?);
    Ok(WorkflowIr {
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
    })
}

pub(crate) fn actionlint_bytes() -> Result<String, RenderError> {
    with_marker(VERSION, "config-variables: []\n")
}

fn simple_job(display: &str, needs: Vec<String>, steps: Vec<Step>) -> Job {
    Job {
        display_name: display.to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs,
        condition: None,
        permissions: None,
        environment: None,
        steps,
    }
}

fn argv_of(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

#[test]
fn triggers_must_be_exact() -> Result<(), RenderError> {
    let ctx = fixture_ctx();
    let mut ir = fixture_ir()?;
    ir.triggers.pull_request_types.pop();
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.triggers.push_branches.push("other".to_owned());
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.triggers.merge_group = false;
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let text = render_workflow_ir(&fixture_ir()?, WorkflowPolicy::ConsumerV1, None, &ctx)?;
    assert!(text.contains("ready_for_review"));
    assert!(text.contains("merge_group:"));
    Ok(())
}

#[test]
fn concurrency_and_label_must_be_exact() -> Result<(), RenderError> {
    let ctx = fixture_ctx();
    let mut ir = fixture_ir()?;
    ir.concurrency.group = "other".to_owned();
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    if let Some(job) = ir.jobs.get_mut("plan") {
        job.runs_on = "ubuntu-24.04".to_owned();
    }
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    for bad in ["ubuntu-latest", "ubuntu-26.04-${{ x }}", "self-hosted", ""] {
        let mut ctx = fixture_ctx();
        ctx.runs_on = bad.to_owned();
        assert!(
            render_workflow_ir(&fixture_ir()?, WorkflowPolicy::ConsumerV1, None, &ctx).is_err(),
            "accepted label {bad:?}"
        );
    }
    Ok(())
}

#[test]
fn final_gate_keeps_exact_name_and_condition() -> Result<(), RenderError> {
    let ctx = fixture_ctx();
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "required".to_owned(),
        Job {
            display_name: "Wrong Name".to_owned(),
            runs_on: LABEL.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::CRATE,
            needs: vec!["plan".to_owned()],
            condition: Some("always()".to_owned()),
            permissions: None,
            environment: None,
            steps: vec![checkout_step(&checkout_pin())?, merge_step()],
        },
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "required".to_owned(),
        Job {
            display_name: "Required".to_owned(),
            runs_on: LABEL.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::REQUIRED,
            needs: vec!["plan".to_owned()],
            condition: None,
            permissions: None,
            environment: None,
            steps: vec![checkout_step(&checkout_pin())?, merge_step()],
        },
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "required".to_owned(),
        Job {
            display_name: "Required".to_owned(),
            runs_on: LABEL.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::REQUIRED,
            needs: vec!["plan".to_owned()],
            condition: Some("always()".to_owned()),
            permissions: Some(Permissions {
                contents: velnor_actions_contract::workflow::permissions::PermissionLevel::Read,
                actions: velnor_actions_contract::workflow::permissions::PermissionLevel::Read,
                pull_requests:
                    velnor_actions_contract::workflow::permissions::PermissionLevel::None,
                id_token: velnor_actions_contract::workflow::permissions::PermissionLevel::None,
            }),
            environment: None,
            steps: vec![checkout_step(&checkout_pin())?, merge_step()],
        },
    );
    let text = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx)?;
    assert!(text.contains("name: Required"));
    assert!(text.contains("if: always()"));
    Ok(())
}

pub(crate) fn task_job(step: Step) -> Job {
    simple_job("Task", vec!["plan".to_owned()], vec![step])
}
