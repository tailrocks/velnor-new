//! Workflow/tree invariant cases (triggers, concurrency, candidate, gates).
use std::collections::BTreeMap;
use velnor_actions_contract::workflow::ir::PermissionLevel;
use velnor_actions_contract::{
    Concurrency, GeneratorValidation, Job, Permissions, Step, StepKind, Trigger, ValidatorKind,
    WorkflowIr, WorkflowPolicy,
};
use velnor_actions_workflow_renderer::{
    ALINT_USES, CANDIDATE_JOB_ID, CONCURRENCY_CANCEL, CONCURRENCY_GROUP, CandidateSpec,
    RenderContext, RenderError, ValidatorCommand, checkout_step, merge_step, plan_step,
    render_tree, render_workflow_ir, shell_step, with_marker,
};

const VERSION: &str = "0.1.0";
const LABEL: &str = "ubuntu-26.04";

fn checkout_pin() -> String {
    format!("actions/checkout@{:040x}", 0)
}

fn fixture_ctx() -> RenderContext {
    RenderContext {
        generator_version: VERSION.to_owned(),
        runs_on: LABEL.to_owned(),
        staged_binary: format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{VERSION}"),
        request_dir: "${{ runner.temp }}/velnor/r1-a1".to_owned(),
        checkout_uses: checkout_pin(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
    }
}

fn exact_triggers() -> Trigger {
    Trigger {
        pull_request_types: ["opened", "synchronize", "reopened", "ready_for_review"]
            .iter()
            .map(ToString::to_string)
            .collect(),
        push_branches: vec!["main".to_owned()],
        merge_group: true,
        workflow_dispatch: None,
        schedule: None,
    }
}

fn exact_concurrency() -> Concurrency {
    Concurrency {
        group: CONCURRENCY_GROUP.to_owned(),
        cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
    }
}

fn plan_job() -> Result<Job, RenderError> {
    Ok(Job {
        display_name: "Plan".to_owned(),
        runs_on: LABEL.to_owned(),
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![checkout_step(&checkout_pin())?, plan_step()],
    })
}

fn validator_commands() -> Vec<ValidatorCommand> {
    [
        ValidatorKind::CargoDeny,
        ValidatorKind::CargoMachete,
        ValidatorKind::Zizmor,
    ]
    .iter()
    .map(|validator| ValidatorCommand {
        validator: *validator,
        name: "Deny".to_owned(),
        argv: vec!["deny".to_owned()],
    })
    .collect()
}

fn fixture_ir() -> Result<WorkflowIr, RenderError> {
    let mut jobs = BTreeMap::new();
    jobs.insert("plan".to_owned(), plan_job()?);
    Ok(WorkflowIr {
        name: "CI".to_owned(),
        triggers: exact_triggers(),
        permissions: Permissions {
            contents: PermissionLevel::Read,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
            actions: PermissionLevel::Read,
        },
        concurrency: exact_concurrency(),
        jobs,
    })
}

fn actionlint_bytes() -> Result<String, RenderError> {
    with_marker(VERSION, "config-variables: []\n")
}

fn simple_job(display: &str, needs: Vec<String>, steps: Vec<Step>) -> Job {
    Job {
        display_name: display.to_owned(),
        runs_on: LABEL.to_owned(),
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
            needs: vec!["plan".to_owned()],
            condition: Some("always()".to_owned()),
            permissions: None,
            environment: None,
            steps: vec![checkout_step(&checkout_pin())?, merge_step()],
        },
    );
    let text = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx)?;
    assert!(text.contains("name: Required"));
    assert!(text.contains("if: always()"));
    Ok(())
}

#[test]
fn renderer_rejects_bare_commands_inside_ir() -> Result<(), RenderError> {
    let ctx = fixture_ctx();
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        simple_job(
            "Task",
            vec!["plan".to_owned()],
            vec![Step {
                name: "Install".to_owned(),
                kind: StepKind::Shell {
                    run: vec!["cargo".to_owned(), "install".to_owned(), "x".to_owned()],
                    env: BTreeMap::new(),
                },
            }],
        ),
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        simple_job(
            "Task",
            vec!["plan".to_owned()],
            vec![shell_step(
                "Focused",
                vec!["true".to_owned()],
                BTreeMap::new(),
            )?],
        ),
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_ok());
    Ok(())
}

#[test]
fn renderer_rejects_unpinned_actions_inside_ir() -> Result<(), RenderError> {
    let ctx = fixture_ctx();
    for (name, uses) in [
        ("Fetch", "actions/checkout@main"),
        ("Run Alint", "asamarts/alint@v0.16.1"),
    ] {
        let mut ir = fixture_ir()?;
        ir.jobs.insert(
            "velnor-task".to_owned(),
            simple_job(
                "Task",
                vec!["plan".to_owned()],
                vec![Step {
                    name: name.to_owned(),
                    kind: StepKind::Action {
                        uses: uses.to_owned(),
                        with: BTreeMap::new(),
                    },
                }],
            ),
        );
        let err = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx)
            .expect_err("unpinned ref must be rejected");
        assert!(
            format!("{err:?}").contains("unpinned_ref"),
            "wrong rejection for {uses}: {err:?}"
        );
    }
    Ok(())
}

#[test]
fn velnor_policy_emits_full_sha_alint_pin() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Bootstrap);
    let text = render_workflow_ir(
        &fixture_ir()?,
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )?;
    assert_eq!(
        ALINT_USES,
        "asamarts/alint@9f9d34ba0eae3888299b9e570f43338b0e7f2cdb"
    );
    assert!(text.contains("  alint:"), "alint job missing:\n{text}");
    assert!(
        text.contains("uses: asamarts/alint@9f9d34ba0eae3888299b9e570f43338b0e7f2cdb"),
        "full-SHA pin missing:\n{text}"
    );
    assert!(
        !text.contains("asamarts/alint@v"),
        "tag ref emitted:\n{text}"
    );
    Ok(())
}

#[test]
fn tree_rejects_unmarked_inputs() -> Result<(), RenderError> {
    let workflow = render_workflow_ir(
        &fixture_ir()?,
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(render_tree(&workflow, "config-variables: []\n", VERSION).is_err());
    assert!(render_tree("name: x\n", &actionlint_bytes()?, VERSION).is_err());
    assert!(render_tree(&workflow, &actionlint_bytes()?, "9.9.9").is_err());
    Ok(())
}
