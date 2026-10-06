//! Context, policy and helper admission shared by both rendering entrypoints.
use super::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP, EXPECTED_PR_TYPES, RenderContext};
use crate::{RenderError, msrv, support};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    Concurrency, Job, Trigger, VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy,
};

/// Validate context/IR plus policy merge and support invariants.
pub(super) fn merged_jobs(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
) -> Result<BTreeMap<String, Job>, RenderError> {
    ctx.validate()?;
    if ctx.preseed && policy != WorkflowPolicy::VelnorRepositoryV1 {
        return Err(RenderError::PolicyRejected {
            policy: "consumer-v1".to_owned(),
            problem: "preseed_requires_velnor_policy".to_owned(),
        });
    }
    ir.validate().map_err(RenderError::Contract)?;
    check_triggers(&ir.triggers)?;
    check_concurrency(&ir.concurrency)?;
    check_single_label(ir, &ctx.runs_on)?;
    let mut jobs = ir.jobs.clone();
    match policy {
        WorkflowPolicy::ConsumerV1 => support::reject_consumer_support(&jobs, support)?,
        WorkflowPolicy::VelnorRepositoryV1 => {
            support::merge_support_jobs(&mut jobs, support, ctx)?;
        }
    }
    crate::receipt_preparation_admission::validate_steps(
        jobs.values().flat_map(|job| job.steps.iter()),
    )?;
    msrv::check_no_msrv(&jobs)?;
    support::check_candidate_invariants(&jobs)?;
    support::check_final_gate(&jobs)?;
    crate::verification_observer::validate_observer_jobs(
        &jobs,
        &ir.triggers,
        &ctx.source_helpers,
        &ctx.generator_version,
    )?;
    support::check_token_hygiene(&jobs, &ctx.source_helpers)?;
    Ok(jobs)
}

pub(super) fn require_nonstrict_readonly(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    if jobs.values().any(crate::early_plan::has_early_plan) {
        return Err(RenderError::InvalidWorkflow(
            "early_plan_requires_strict_admission".to_owned(),
        ));
    }
    if jobs.values().any(|job| {
        job.source_producer.is_some()
            || job.tool_producer.is_some()
            || job.steps.iter().any(|step| {
                crate::cache_tool_paths::owned_transport(step)
                    || matches!(&step.kind,
            velnor_actions_contract::StepKind::SourceBoundHelper { invocation, .. }
            if invocation.descriptor().operation().is_source_producer())
            })
    }) {
        return Err(RenderError::InvalidWorkflow(
            "source_producer_requires_strict_admission".to_owned(),
        ));
    }
    Ok(())
}

/// Require the exact trigger shape: 4 PR types, one push branch, merge group.
fn check_triggers(triggers: &Trigger) -> Result<(), RenderError> {
    let expected: Vec<String> = EXPECTED_PR_TYPES.iter().map(ToString::to_string).collect();
    if triggers.pull_request_types != expected {
        return Err(RenderError::InvalidWorkflow("bad_pr_triggers".to_owned()));
    }
    let branch_ok = triggers.push_branches.len() == 1
        && triggers
            .push_branches
            .first()
            .is_some_and(|branch| velnor_actions_contract::is_valid_branch_name(branch));
    if !branch_ok {
        return Err(RenderError::InvalidWorkflow("bad_push_branch".to_owned()));
    }
    if !triggers.merge_group {
        return Err(RenderError::InvalidWorkflow(
            "missing_merge_group".to_owned(),
        ));
    }
    Ok(())
}

/// Require the exact concurrency group plus PR-only cancel.
fn check_concurrency(concurrency: &Concurrency) -> Result<(), RenderError> {
    if concurrency.group != CONCURRENCY_GROUP
        || concurrency.cancel_in_progress != CONCURRENCY_CANCEL
    {
        return Err(RenderError::InvalidWorkflow("bad_concurrency".to_owned()));
    }
    Ok(())
}

/// Require every job to use the single context label.
fn check_single_label(ir: &WorkflowIr, label: &str) -> Result<(), RenderError> {
    for (id, job) in &ir.jobs {
        if job.runs_on != label {
            return Err(RenderError::InvalidWorkflow(format!("label_mismatch:{id}")));
        }
    }
    Ok(())
}
