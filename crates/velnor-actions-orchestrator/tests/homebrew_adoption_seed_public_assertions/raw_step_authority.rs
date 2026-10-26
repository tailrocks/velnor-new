// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, io};

use velnor_actions_contract::{Job, Step, StepKind, StepRole, WorkflowPolicy};
use velnor_actions_orchestrator::{GenerationPreparation, OrchestratorError, finalized_jobs};
use velnor_actions_workflow_renderer::{
    RenderError,
    render::{FINAL_JOB_ID, PLAN_JOB_ID, render_workflow_ir},
};

#[derive(Clone, Copy)]
enum Target {
    Role(StepRole),
    Internal(&'static str),
}

#[derive(Clone, Copy)]
enum Field {
    Uses,
    With,
    Env,
    Condition,
    Name,
}

pub(super) fn assert_raw_step_authority(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    finalized_jobs(preparation)?;
    assert_raw_render(preparation, policy)?;
    assert_optional_canonical_setup(preparation, policy)?;
    assert_finalizer_rejects_untrusted_mise_setup(preparation, policy)?;
    for (job, target, field) in required_mutations() {
        assert_rejected(
            preparation,
            policy,
            job,
            target,
            field,
            "required_verdict_candidate_execution",
        )?;
    }
    if policy == WorkflowPolicy::VelnorRepositoryV1 {
        for (job, target, field) in plan_mutations() {
            assert_rejected(
                preparation,
                policy,
                job,
                target,
                field,
                "trusted_plan_candidate_execution",
            )?;
        }
    }
    Ok(())
}

fn assert_optional_canonical_setup(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    let jobs = finalized_jobs(preparation)?;
    let required_setup = role_step(&jobs, FINAL_JOB_ID, StepRole::MiseSetup)?.clone();
    let mut changed = preparation.clone();
    let required = changed
        .workflow
        .ir
        .jobs
        .get_mut(FINAL_JOB_ID)
        .ok_or_else(|| io::Error::other("workflow omitted Required"))?;
    insert_after_checkout(required, required_setup)?;
    if policy == WorkflowPolicy::VelnorRepositoryV1 {
        let plan_setup = role_step(&jobs, PLAN_JOB_ID, StepRole::MiseSetup)?.clone();
        let plan = changed
            .workflow
            .ir
            .jobs
            .get_mut(PLAN_JOB_ID)
            .ok_or_else(|| io::Error::other("workflow omitted Plan"))?;
        insert_after_checkout(plan, plan_setup)?;
    }
    assert_raw_render(&changed, policy)?;
    finalized_jobs(&changed)?;
    Ok(())
}

fn insert_after_checkout(job: &mut Job, setup: Step) -> Result<(), Box<dyn Error>> {
    let checkout = job
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::Checkout))
        .ok_or_else(|| io::Error::other("authority job omitted Checkout"))?;
    job.steps.insert(checkout + 1, setup);
    Ok(())
}

fn assert_rejected(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
    job: &str,
    target: Target,
    field: Field,
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    let mut changed = preparation.clone();
    if matches!(target, Target::Role(StepRole::MiseSetup)) {
        insert_canonical_setup(&mut changed, policy)?;
    }
    mutate(&mut changed, job, target, field)?;
    let raw_error = render_workflow_ir(
        &changed.workflow.ir,
        policy,
        changed.workflow.support.as_ref(),
        &changed.workflow.context,
    )
    .err()
    .ok_or_else(|| io::Error::other(format!("raw rendering accepted {job} authority mutation")))?;
    assert!(matches!(raw_error, RenderError::InvalidWorkflow(problem) if problem == expected));
    let strict_error = finalized_jobs(&changed).err().ok_or_else(|| {
        io::Error::other(format!("finalization accepted {job} authority mutation"))
    })?;
    assert!(
        matches!(strict_error, OrchestratorError::Render { problem } if problem == format!("invalid workflow: {expected}"))
    );
    Ok(())
}

fn insert_canonical_setup(
    preparation: &mut GenerationPreparation,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    let jobs = finalized_jobs(preparation)?;
    let setup = role_step(&jobs, FINAL_JOB_ID, StepRole::MiseSetup)?.clone();
    let required = preparation
        .workflow
        .ir
        .jobs
        .get_mut(FINAL_JOB_ID)
        .ok_or_else(|| io::Error::other("workflow omitted Required"))?;
    insert_after_checkout(required, setup)?;
    if policy == WorkflowPolicy::VelnorRepositoryV1 {
        let setup = role_step(&jobs, PLAN_JOB_ID, StepRole::MiseSetup)?.clone();
        let plan = preparation
            .workflow
            .ir
            .jobs
            .get_mut(PLAN_JOB_ID)
            .ok_or_else(|| io::Error::other("workflow omitted Plan"))?;
        insert_after_checkout(plan, setup)?;
    }
    Ok(())
}

fn assert_finalizer_rejects_untrusted_mise_setup(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    let jobs = finalized_jobs(preparation)?;
    let canonical = role_step(&jobs, FINAL_JOB_ID, StepRole::MiseSetup)?;
    let StepKind::Action { uses, with, .. } = &canonical.kind else {
        return Err("canonical Mise setup is not an action".into());
    };
    let version = with
        .get("version")
        .ok_or("canonical Mise setup omitted version")?
        .clone();
    let action_prefix = uses
        .split_once('@')
        .map_or(uses.as_str(), |(prefix, _)| prefix);
    for (forged, message) in [
        (
            velnor_actions_workflow_renderer::MiseSetup {
                uses: format!("{action_prefix}@0000000000000000000000000000000000000000"),
                version: version.clone(),
                sha256: with
                    .get("sha256")
                    .ok_or("canonical Mise setup omitted digest")?
                    .clone(),
            },
            "renderer finalization accepted a noncanonical Mise action pin",
        ),
        (
            velnor_actions_workflow_renderer::MiseSetup {
                uses: uses.clone(),
                version,
                sha256: "0".repeat(64),
            },
            "renderer finalization accepted a noncanonical Mise binary digest",
        ),
    ] {
        let error = velnor_actions_workflow_renderer::finalize_jobs(
            &preparation.workflow.ir,
            policy,
            preparation.workflow.support.as_ref(),
            &preparation.workflow.context,
            &forged,
        )
        .err()
        .ok_or(message)?;
        assert!(
            matches!(error, RenderError::InvalidWorkflow(problem) if problem == "required_verdict_candidate_execution")
        );
    }
    Ok(())
}

fn assert_raw_render(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    render_workflow_ir(
        &preparation.workflow.ir,
        policy,
        preparation.workflow.support.as_ref(),
        &preparation.workflow.context,
    )?;
    Ok(())
}

fn required_mutations() -> Vec<(&'static str, Target, Field)> {
    vec![
        (FINAL_JOB_ID, Target::Role(StepRole::Checkout), Field::Uses),
        (FINAL_JOB_ID, Target::Role(StepRole::Checkout), Field::With),
        (FINAL_JOB_ID, Target::Role(StepRole::Checkout), Field::Env),
        (
            FINAL_JOB_ID,
            Target::Role(StepRole::Checkout),
            Field::Condition,
        ),
        (FINAL_JOB_ID, Target::Role(StepRole::Checkout), Field::Name),
        (FINAL_JOB_ID, Target::Role(StepRole::MiseSetup), Field::Uses),
        (FINAL_JOB_ID, Target::Role(StepRole::MiseSetup), Field::With),
        (FINAL_JOB_ID, Target::Role(StepRole::MiseSetup), Field::Env),
        (
            FINAL_JOB_ID,
            Target::Role(StepRole::MiseSetup),
            Field::Condition,
        ),
        (FINAL_JOB_ID, Target::Role(StepRole::MiseSetup), Field::Name),
        (FINAL_JOB_ID, Target::Internal("merge-v1"), Field::Env),
        (FINAL_JOB_ID, Target::Internal("merge-v1"), Field::Condition),
    ]
}

fn plan_mutations() -> Vec<(&'static str, Target, Field)> {
    vec![
        (PLAN_JOB_ID, Target::Role(StepRole::Checkout), Field::Uses),
        (PLAN_JOB_ID, Target::Role(StepRole::Checkout), Field::With),
        (PLAN_JOB_ID, Target::Role(StepRole::Checkout), Field::Env),
        (
            PLAN_JOB_ID,
            Target::Role(StepRole::Checkout),
            Field::Condition,
        ),
        (PLAN_JOB_ID, Target::Role(StepRole::Checkout), Field::Name),
        (PLAN_JOB_ID, Target::Role(StepRole::MiseSetup), Field::Uses),
        (PLAN_JOB_ID, Target::Role(StepRole::MiseSetup), Field::With),
        (PLAN_JOB_ID, Target::Role(StepRole::MiseSetup), Field::Env),
        (
            PLAN_JOB_ID,
            Target::Role(StepRole::MiseSetup),
            Field::Condition,
        ),
        (PLAN_JOB_ID, Target::Role(StepRole::MiseSetup), Field::Name),
        (PLAN_JOB_ID, Target::Internal("plan-v1"), Field::Env),
        (PLAN_JOB_ID, Target::Internal("plan-v1"), Field::Condition),
    ]
}

fn mutate(
    preparation: &mut GenerationPreparation,
    job_id: &str,
    target: Target,
    field: Field,
) -> Result<(), Box<dyn Error>> {
    let job = preparation
        .workflow
        .ir
        .jobs
        .get_mut(job_id)
        .ok_or_else(|| io::Error::other(format!("workflow omitted {job_id}")))?;
    let step = job
        .steps
        .iter_mut()
        .find(|step| match target {
            Target::Role(role) => step.role == Some(role),
            Target::Internal(operation) => matches!(
                &step.kind,
                StepKind::Internal { operation: actual, .. } if actual == operation
            ),
        })
        .ok_or_else(|| io::Error::other(format!("{job_id} omitted mutation target")))?;
    match field {
        Field::Uses => {
            let StepKind::Action { uses, .. } = &mut step.kind else {
                return Err("uses mutation target is not an action".into());
            };
            let prefix = uses
                .split_once('@')
                .map_or(uses.as_str(), |(prefix, _)| prefix);
            *uses = format!("{prefix}@0000000000000000000000000000000000000000");
        }
        Field::With => match &mut step.kind {
            StepKind::Action { with, .. } => {
                with.insert("candidate-input".to_owned(), "true".to_owned());
            }
            _ => return Err("with mutation target is not an action".into()),
        },
        Field::Env => match &mut step.kind {
            StepKind::Action { env, .. }
            | StepKind::Shell { env, .. }
            | StepKind::Internal { env, .. } => {
                env.insert("BASH_ENV".to_owned(), "candidate-hook.sh".to_owned());
            }
            _ => return Err("env mutation target has no environment".into()),
        },
        Field::Condition => step.condition = Some("always()".to_owned()),
        Field::Name => step.name.push_str(" candidate"),
    }
    Ok(())
}

fn role_step<'a>(
    jobs: &'a std::collections::BTreeMap<String, Job>,
    job: &str,
    role: StepRole,
) -> Result<&'a Step, Box<dyn Error>> {
    jobs.get(job)
        .and_then(|job| job.steps.iter().find(|step| step.role == Some(role)))
        .ok_or_else(|| io::Error::other(format!("{job} omitted {role:?}")).into())
}
