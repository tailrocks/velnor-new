//! Workflow/tree invariant cases (triggers, concurrency, candidate, gates).
use std::collections::BTreeMap;
use velnor_actions_contract::{
    Concurrency, GeneratorValidation, Job, Permissions, Step, StepKind, Trigger, WorkflowIr,
    WorkflowPolicy,
};
use velnor_actions_workflow_renderer::{
    CANDIDATE_JOB_ID, CONCURRENCY_CANCEL, CONCURRENCY_GROUP, CandidateSpec, PolicyCommand,
    RenderContext, RenderError, checkout_step, merge_step, plan_step, render_tree,
    render_workflow_ir, shell_step, with_marker,
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
        request_dir: "$RUNNER_TEMP/velnor/r1-a1".to_owned(),
        checkout_uses: checkout_pin(),
        policy_commands: Vec::new(),
        candidate: None,
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
        display_name: "Velnor Plan".to_owned(),
        runs_on: LABEL.to_owned(),
        needs: Vec::new(),
        condition: None,
        steps: vec![checkout_step(&checkout_pin())?, plan_step()],
    })
}

fn fixture_ir() -> Result<WorkflowIr, RenderError> {
    let mut jobs = BTreeMap::new();
    jobs.insert("velnor-plan".to_owned(), plan_job()?);
    Ok(WorkflowIr {
        name: "CI".to_owned(),
        triggers: exact_triggers(),
        permissions: Permissions {
            contents: "read".to_owned(),
            actions: "read".to_owned(),
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
    if let Some(job) = ir.jobs.get_mut("velnor-plan") {
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
            vec!["velnor-plan".to_owned()],
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
            vec!["velnor-plan".to_owned()],
            vec![checkout_step(&checkout_pin())?],
        ),
    );
    assert!(render_workflow_ir(&thief, policy, None, &ctx).is_err());
    Ok(())
}

#[test]
fn candidate_job_renders_with_plan_dependency() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.policy_commands = vec![PolicyCommand {
        name: "Deny".to_owned(),
        argv: vec!["deny".to_owned()],
    }];
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
    assert!(text.contains("velnor-candidate:"));
    assert!(text.contains("velnor-plan"));
    assert!(text.contains("velnor-release:"));
    assert!(text.contains("ref_protected"));
    assert!(text.contains("actions/download-artifact@"));
    Ok(())
}

#[test]
fn final_gate_keeps_exact_name_and_condition() -> Result<(), RenderError> {
    let ctx = fixture_ctx();
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-final".to_owned(),
        Job {
            display_name: "Wrong Name".to_owned(),
            runs_on: LABEL.to_owned(),
            needs: vec!["velnor-plan".to_owned()],
            condition: Some("always()".to_owned()),
            steps: vec![checkout_step(&checkout_pin())?, merge_step()],
        },
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-final".to_owned(),
        Job {
            display_name: "Velnor / Required".to_owned(),
            runs_on: LABEL.to_owned(),
            needs: vec!["velnor-plan".to_owned()],
            condition: None,
            steps: vec![checkout_step(&checkout_pin())?, merge_step()],
        },
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-final".to_owned(),
        Job {
            display_name: "Velnor / Required".to_owned(),
            runs_on: LABEL.to_owned(),
            needs: vec!["velnor-plan".to_owned()],
            condition: Some("always()".to_owned()),
            steps: vec![checkout_step(&checkout_pin())?, merge_step()],
        },
    );
    let text = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx)?;
    assert!(text.contains("name: Velnor / Required"));
    assert!(text.contains("if: always()"));
    Ok(())
}

#[test]
fn renderer_rejects_unvalidated_steps_inside_ir() -> Result<(), RenderError> {
    let ctx = fixture_ctx();
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        Job {
            display_name: "Task".to_owned(),
            runs_on: LABEL.to_owned(),
            needs: vec!["velnor-plan".to_owned()],
            condition: None,
            steps: vec![Step {
                name: "Install".to_owned(),
                kind: StepKind::Shell {
                    run: vec!["cargo".to_owned(), "install".to_owned(), "x".to_owned()],
                    env: BTreeMap::new(),
                },
            }],
        },
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        Job {
            display_name: "Task".to_owned(),
            runs_on: LABEL.to_owned(),
            needs: vec!["velnor-plan".to_owned()],
            condition: None,
            steps: vec![Step {
                name: "Fetch".to_owned(),
                kind: StepKind::Action {
                    uses: "actions/checkout@main".to_owned(),
                    with: BTreeMap::new(),
                },
            }],
        },
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        Job {
            display_name: "Task".to_owned(),
            runs_on: LABEL.to_owned(),
            needs: vec!["velnor-plan".to_owned()],
            condition: None,
            steps: vec![shell_step(
                "Focused",
                vec!["true".to_owned()],
                BTreeMap::new(),
            )?],
        },
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_ok());
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
