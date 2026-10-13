//! Workflow/tree invariant cases (triggers, concurrency, candidate, gates).
use std::collections::BTreeMap;
use velnor_actions_contract_config::{GeneratorValidation, ValidatorKind, WorkflowPolicy};
use velnor_actions_contract_workflow::{
    Concurrency, Job, JobTimeout, PermissionLevel, Permissions, Step, StepKind, Trigger, WorkflowIr,
};
use velnor_actions_workflow_jobs::{
    CANDIDATE_JOB_ID, CONCURRENCY_CANCEL, CONCURRENCY_GROUP, CandidateSpec, RenderContext,
    ValidatorCommand,
};
use velnor_actions_workflow_renderer::render_workflow_ir;
use velnor_actions_workflow_steps::{RenderError, checkout_step, merge_step, plan_step};

use crate::impl_renderer_fixtures::policy_pin;

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
        rust_policy: Some(policy_pin()),
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
    velnor_actions_workflow_tree::with_marker(VERSION, "config-variables: []\n")
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
fn candidate_never_plans_and_lock_matches_catalog_per_target() -> Result<(), RenderError> {
    let ctx = fixture_ctx();
    let policy = WorkflowPolicy::VelnorRepositoryV1;
    let mut lonely = fixture_ir()?;
    lonely.jobs.insert(
        CANDIDATE_JOB_ID.to_owned(),
        simple_job("Lonely", Vec::new(), vec![checkout_step(&checkout_pin())?]),
    );
    assert!(render_workflow_ir(&lonely, policy, None, &ctx).is_err());
    let mut planner = fixture_ir()?;
    planner.jobs.insert(
        CANDIDATE_JOB_ID.to_owned(),
        simple_job(
            "Planning candidate",
            vec!["plan".to_owned()],
            vec![checkout_step(&checkout_pin())?, plan_step()],
        ),
    );
    assert!(render_workflow_ir(&planner, policy, None, &ctx).is_err());
    let mut thief = fixture_ir()?;
    thief.jobs.insert(
        "velnor-task".to_owned(),
        simple_job(
            "Task",
            vec![CANDIDATE_JOB_ID.to_owned()],
            vec![checkout_step(&checkout_pin())?],
        ),
    );
    thief.jobs.insert(
        CANDIDATE_JOB_ID.to_owned(),
        simple_job(
            "Candidate",
            vec!["plan".to_owned()],
            vec![checkout_step(&checkout_pin())?],
        ),
    );
    assert!(render_workflow_ir(&thief, policy, None, &ctx).is_err());
    Ok(())
}

#[test]
fn candidate_job_renders_with_plan_dependency() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    ctx.candidate = Some(CandidateSpec {
        build: argv_of(&[
            "mise",
            "exec",
            "rust@1.98.1",
            "mr-boxington@1.19.0",
            "--",
            "build",
        ]),
        qualify: argv_of(&["sh", "-c", "plan-and-check"]),
    });
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    let text = render_workflow_ir(
        &fixture_ir()?,
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )?;
    assert!(text.contains("candidate:"));
    assert!(text.contains("plan"));
    assert!(text.contains("release:"));
    assert!(text.contains("ref_protected"));
    assert!(text.contains("actions/download-artifact@"));
    // S1: the build compiles PR source and qualification executes the
    // PR-built binary, so both steps unset runner credentials first.
    assert!(
        text.contains("env -u ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
        "build must unset:\n{text}"
    );
    assert!(
        text.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
        "qualify must unset:\n{text}"
    );
    assert!(
        text.contains("GITHUB_TOKEN: \"\""),
        "candidate steps must scrub:\n{text}"
    );
    Ok(())
}

#[test]
fn candidate_qualify_rejects_non_shell_vectors() {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    ctx.candidate = Some(CandidateSpec {
        build: argv_of(&[
            "mise",
            "exec",
            "rust@1.98.1",
            "mr-boxington@1.19.0",
            "--",
            "build",
        ]),
        qualify: argv_of(&["mise", "run", "qualify"]),
    });
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    let ir = fixture_ir().expect("ir");
    let err = render_workflow_ir(
        &ir,
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )
    .expect_err("non-shell qualify must fail");
    assert!(
        format!("{err:?}").contains("qualify_without_unset"),
        "wrong rejection: {err:?}"
    );
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
                contents: PermissionLevel::Read,
                actions: PermissionLevel::Read,
                pull_requests: PermissionLevel::None,
                id_token: PermissionLevel::None,
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

#[test]
fn renderer_rejects_bare_commands_inside_ir() -> Result<(), RenderError> {
    let ctx = fixture_ctx();
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        task_job(Step {
            name: "Install".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Shell {
                run: vec!["cargo".to_owned(), "install".to_owned(), "x".to_owned()],
                env: BTreeMap::new(),
            },
        }),
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        task_job(Step {
            name: "Fetch".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Action {
                uses: "actions/checkout@main".to_owned(),
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            },
        }),
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        task_job(Step {
            name: "Run Alint".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Action {
                uses: "asamarts/alint@v0.16.1".to_owned(),
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            },
        }),
    );
    let err = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx)
        .expect_err("alint tag ref must be rejected");
    assert!(
        format!("{err:?}").contains("unpinned_ref"),
        "wrong rejection: {err:?}"
    );
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        task_job(velnor_actions_workflow_steps::shell_step(
            "Focused",
            vec!["true".to_owned()],
            BTreeMap::new(),
        )?),
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_ok());
    Ok(())
}
