//! Policy gating and Velnor support-job construction.
//!
//! Consumer policy rejects support IR; Velnor policy merges typed Alint,
//! policy, and optional candidate jobs. No toolchain-qualification job exists.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, PolicyJob, Step, StepKind, VelnorSupportWorkflow};

use crate::{
    RenderError,
    render::{
        ALINT_JOB_ID, ALINT_USES, CANDIDATE_JOB_ID, CandidateSpec, FINAL_CONDITION,
        FINAL_DISPLAY_NAME, FINAL_JOB_ID, PLAN_JOB_ID, POLICY_JOB_ID, RenderContext, TASK_JOB_ID,
    },
    steps,
};

/// Consumer policy: reject support IR and Velnor-only job IDs.
pub(crate) fn reject_consumer_support(
    jobs: &BTreeMap<String, Job>,
    support: Option<&VelnorSupportWorkflow>,
) -> Result<(), RenderError> {
    if let Some(workflow) = support
        && (!workflow.policy_jobs.is_empty() || workflow.candidate_validation)
    {
        return Err(RenderError::PolicyRejected {
            policy: "consumer-v1".to_owned(),
            problem: "support_ir_rejected".to_owned(),
        });
    }
    for id in [ALINT_JOB_ID, POLICY_JOB_ID, CANDIDATE_JOB_ID] {
        if jobs.contains_key(id) {
            return Err(RenderError::PolicyRejected {
                policy: "consumer-v1".to_owned(),
                problem: format!("forbidden_job:{id}"),
            });
        }
    }
    Ok(())
}

/// Velnor policy: merge typed Alint, policy, and optional candidate jobs.
/// The exhaustive `PolicyJob` match is the whole support set: Alint plus
/// policy only, with no toolchain-qualification job.
pub(crate) fn merge_support_jobs(
    jobs: &mut BTreeMap<String, Job>,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
) -> Result<(), RenderError> {
    let Some(workflow) = support else {
        return Ok(());
    };
    let mut want_alint = false;
    let mut want_policy = false;
    for job in &workflow.policy_jobs {
        match job {
            PolicyJob::Alint if !want_alint => want_alint = true,
            PolicyJob::Policy if !want_policy => want_policy = true,
            _ => {
                return Err(RenderError::PolicyRejected {
                    policy: "velnor-repository-v1".to_owned(),
                    problem: "duplicate_policy_job".to_owned(),
                });
            }
        }
    }
    if want_alint {
        insert_support_job(jobs, ALINT_JOB_ID, alint_job(ctx)?)?;
    }
    if want_policy {
        insert_support_job(jobs, POLICY_JOB_ID, policy_job(ctx)?)?;
    }
    if workflow.candidate_validation {
        let Some(spec) = &ctx.candidate else {
            return Err(RenderError::PolicyRejected {
                policy: "velnor-repository-v1".to_owned(),
                problem: "candidate_without_spec".to_owned(),
            });
        };
        insert_support_job(jobs, CANDIDATE_JOB_ID, candidate_job(ctx, spec)?)?;
    }
    Ok(())
}

/// Insert a support job, failing on ID collision with IR jobs.
pub(crate) fn insert_support_job(
    jobs: &mut BTreeMap<String, Job>,
    id: &str,
    job: Job,
) -> Result<(), RenderError> {
    if jobs.contains_key(id) {
        return Err(RenderError::PolicyRejected {
            policy: "velnor-repository-v1".to_owned(),
            problem: format!("job_collision:{id}"),
        });
    }
    jobs.insert(id.to_owned(), job);
    Ok(())
}

/// Fixed Alint job: checkout plus the pinned-tag Alint action.
pub(crate) fn alint_job(ctx: &RenderContext) -> Result<Job, RenderError> {
    let checkout = steps::checkout_step(&ctx.checkout_uses)?;
    let mut with = BTreeMap::new();
    with.insert("path".to_owned(), ".".to_owned());
    with.insert("config".to_owned(), ".alint.yml".to_owned());
    with.insert("format".to_owned(), "github".to_owned());
    with.insert("fail-on-warning".to_owned(), "true".to_owned());
    steps::scan_for_private_subcommands(ALINT_USES)?;
    Ok(Job {
        display_name: "Velnor Alint".to_owned(),
        runs_on: ctx.runs_on.clone(),
        needs: Vec::new(),
        condition: None,
        steps: vec![
            checkout,
            Step {
                name: "Run Alint".to_owned(),
                kind: StepKind::Action {
                    uses: ALINT_USES.to_owned(),
                    with,
                },
            },
        ],
    })
}

/// Fixed policy job: checkout plus caller-supplied validated commands.
pub(crate) fn policy_job(ctx: &RenderContext) -> Result<Job, RenderError> {
    if ctx.policy_commands.is_empty() {
        return Err(RenderError::InvalidWorkflow(
            "policy_without_commands".to_owned(),
        ));
    }
    let mut rendered = Vec::with_capacity(ctx.policy_commands.len() + 1);
    rendered.push(steps::checkout_step(&ctx.checkout_uses)?);
    for command in &ctx.policy_commands {
        rendered.push(steps::shell_step(
            &command.name,
            command.argv.clone(),
            BTreeMap::new(),
        )?);
    }
    Ok(Job {
        display_name: "Velnor Policy".to_owned(),
        runs_on: ctx.runs_on.clone(),
        needs: Vec::new(),
        condition: None,
        steps: rendered,
    })
}

/// Candidate job: checkout, fixed build, fixed qualification; needs plan.
pub(crate) fn candidate_job(ctx: &RenderContext, spec: &CandidateSpec) -> Result<Job, RenderError> {
    Ok(Job {
        display_name: "Velnor Candidate".to_owned(),
        runs_on: ctx.runs_on.clone(),
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        steps: vec![
            steps::checkout_step(&ctx.checkout_uses)?,
            steps::shell_step("Build candidate", spec.build.clone(), BTreeMap::new())?,
            steps::shell_step("Qualify candidate", spec.qualify.clone(), BTreeMap::new())?,
        ],
    })
}

/// Candidate never plans: it needs plan, holds no plan step, feeds no task.
pub(crate) fn check_candidate_invariants(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    if let Some(candidate) = jobs.get(CANDIDATE_JOB_ID) {
        if !candidate.needs.contains(&PLAN_JOB_ID.to_owned()) {
            return Err(RenderError::InvalidWorkflow(
                "candidate_must_need_plan".to_owned(),
            ));
        }
        for step in &candidate.steps {
            if let StepKind::Internal { operation } = &step.kind
                && operation == steps::PLAN_OPERATION
            {
                return Err(RenderError::InvalidWorkflow(
                    "candidate_must_not_plan".to_owned(),
                ));
            }
        }
    }
    if let Some(task) = jobs.get(TASK_JOB_ID)
        && task.needs.contains(&CANDIDATE_JOB_ID.to_owned())
    {
        return Err(RenderError::InvalidWorkflow(
            "task_must_not_consume_candidate".to_owned(),
        ));
    }
    Ok(())
}

/// Final gate keeps the exact required-check name and `always()` condition.
pub(crate) fn check_final_gate(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    if let Some(final_job) = jobs.get(FINAL_JOB_ID) {
        if final_job.display_name != FINAL_DISPLAY_NAME {
            return Err(RenderError::InvalidWorkflow("bad_final_name".to_owned()));
        }
        if final_job.condition.as_deref() != Some(FINAL_CONDITION) {
            return Err(RenderError::InvalidWorkflow(
                "bad_final_condition".to_owned(),
            ));
        }
    }
    Ok(())
}
