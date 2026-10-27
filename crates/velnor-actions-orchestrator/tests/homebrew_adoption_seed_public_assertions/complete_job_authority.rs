// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, error::Error, io};

use velnor_actions_contract::{Job, JobTimeout, PermissionLevel, WorkflowPolicy};
use velnor_actions_orchestrator::{GenerationPreparation, OrchestratorError, finalized_jobs};
use velnor_actions_workflow_renderer::{
    RenderError,
    render::{FINAL_JOB_ID, PLAN_JOB_ID, render_workflow_ir},
};

pub(crate) fn assert_complete_job_authority(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    finalized_jobs(preparation)?;
    render_workflow_ir(
        &preparation.workflow.ir,
        policy,
        preparation.workflow.support.as_ref(),
        &preparation.workflow.context,
    )?;

    assert_required_metadata(preparation, policy)?;
    if policy == WorkflowPolicy::VelnorRepositoryV1 {
        assert_producer_metadata(preparation, policy)?;
    }
    Ok(())
}

/// Adding `actions: write` to a job that checks out source is rejected at
/// generic validation (before render), so the render-rejection path is
/// unreachable for this mutation. The tamper is still caught, fail-closed.
fn assert_checkout_write_mutation_rejected_at_generic(
    preparation: &GenerationPreparation,
    job_id: &str,
    field: &str,
) -> Result<(), Box<dyn Error>> {
    let mut changed = preparation.clone();
    let job = changed
        .workflow
        .ir
        .jobs
        .get_mut(job_id)
        .ok_or_else(|| io::Error::other(format!("preparation omitted {job_id}")))?;
    job.permissions.get_or_insert_with(Default::default).actions = PermissionLevel::Write;
    let error = changed.workflow.ir.validate().err().ok_or_else(|| {
        io::Error::other(format!(
            "generic validator accepted {field} checkout-write mutation"
        ))
    })?;
    assert!(
        error
            .to_string()
            .contains("actions_write_requires_checkout_free"),
        "unexpected generic rejection for {field}: {error}"
    );
    Ok(())
}

fn assert_required_metadata(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    let required_error = "required_verdict_candidate_execution";
    assert_checkout_write_mutation_rejected_at_generic(preparation, FINAL_JOB_ID, "permissions")?;
    assert_job_mutation_rejected(
        preparation,
        policy,
        FINAL_JOB_ID,
        required_error,
        "runner",
        |job| "ubuntu-24.04".clone_into(&mut job.runs_on),
    )?;
    assert_job_mutation_rejected(
        preparation,
        policy,
        FINAL_JOB_ID,
        required_error,
        "timeout",
        |job| {
            job.timeout_minutes = JobTimeout::PRODUCT_COMPILER;
        },
    )?;
    assert_job_mutation_rejected(
        preparation,
        policy,
        FINAL_JOB_ID,
        required_error,
        "needs",
        |job| job.needs.retain(|need| need != "jactionlint"),
    )?;
    assert_job_mutation_rejected(
        preparation,
        policy,
        FINAL_JOB_ID,
        required_error,
        "condition",
        |job| {
            job.condition = Some("always()".to_owned());
        },
    )?;
    assert_job_mutation_rejected(
        preparation,
        policy,
        FINAL_JOB_ID,
        required_error,
        "environment",
        |job| job.environment = Some("review-environment".to_owned()),
    )?;
    Ok(())
}

fn assert_producer_metadata(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    let plan_error = "trusted_plan_candidate_execution";
    assert_checkout_write_mutation_rejected_at_generic(preparation, PLAN_JOB_ID, "permissions")?;
    assert_job_mutation_rejected(
        preparation,
        policy,
        PLAN_JOB_ID,
        plan_error,
        "runner",
        |job| "ubuntu-24.04".clone_into(&mut job.runs_on),
    )?;
    assert_job_mutation_rejected(
        preparation,
        policy,
        PLAN_JOB_ID,
        plan_error,
        "timeout",
        |job| job.timeout_minutes = JobTimeout::PRODUCT_COMPILER,
    )?;
    assert_job_mutation_rejected(
        preparation,
        policy,
        PLAN_JOB_ID,
        plan_error,
        "condition",
        |job| job.condition = Some("always()".to_owned()),
    )?;
    assert_job_mutation_rejected(
        preparation,
        policy,
        PLAN_JOB_ID,
        plan_error,
        "environment",
        |job| job.environment = Some("review-environment".to_owned()),
    )?;

    let mut changed = preparation.clone();
    let tail = velnor_actions_workflow_renderer::ambient_shell_step(
        "Candidate tail",
        vec!["printf".to_owned(), "candidate".to_owned()],
        BTreeMap::new(),
    )?;
    changed
        .workflow
        .ir
        .jobs
        .get_mut(PLAN_JOB_ID)
        .ok_or_else(|| io::Error::other("producer preparation omitted Plan"))?
        .steps
        .push(tail);
    assert_generic_validation_accepts(&changed, "producer Plan tail")?;
    assert_rejected(&changed, policy, plan_error, "producer Plan tail")
}

fn assert_job_mutation_rejected(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
    job_id: &str,
    expected: &str,
    field: &str,
    mutate: impl FnOnce(&mut Job),
) -> Result<(), Box<dyn Error>> {
    let mut changed = preparation.clone();
    let job = changed
        .workflow
        .ir
        .jobs
        .get_mut(job_id)
        .ok_or_else(|| io::Error::other(format!("preparation omitted {job_id}")))?;
    mutate(job);
    assert_generic_validation_accepts(&changed, field)?;
    assert_rejected(&changed, policy, expected, field)
}

fn assert_generic_validation_accepts(
    preparation: &GenerationPreparation,
    field: &str,
) -> Result<(), Box<dyn Error>> {
    preparation.workflow.ir.validate().map_err(|error| {
        io::Error::other(format!("generic validator rejected {field}: {error}"))
    })?;
    Ok(())
}

fn assert_rejected(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
    expected: &str,
    field: &str,
) -> Result<(), Box<dyn Error>> {
    let raw_error = render_workflow_ir(
        &preparation.workflow.ir,
        policy,
        preparation.workflow.support.as_ref(),
        &preparation.workflow.context,
    )
    .err()
    .ok_or_else(|| io::Error::other(format!("renderer accepted changed authority Job: {field}")))?;
    assert!(
        matches!(&raw_error, RenderError::InvalidWorkflow(problem) if problem == expected),
        "unexpected authority rejection for {field}: {raw_error:?}; expected {expected}"
    );
    let error = finalized_jobs(preparation)
        .err()
        .ok_or_else(|| io::Error::other("finalizer accepted changed authority Job"))?;
    assert!(
        matches!(error, OrchestratorError::Render { problem } if problem == format!("invalid workflow: {expected}"))
    );
    Ok(())
}
