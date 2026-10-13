//! Plan-job `Format` step (workflow contract §3 plan step 5).
//!
//! Formatting runs in `plan` per selected stack config through
//! pinned Mise; the fixed argv arrives from the orchestrator's Mise
//! vectors. The renderer validates the Mise shape and anchors the step
//! between helper staging and the freshness/plan closure.

use std::collections::BTreeMap;

use velnor_actions_contract_workflow::{Job, Step, StepKind, StepRole};

use crate::render::PLAN_JOB_ID;
use velnor_actions_workflow_steps::{RenderError, steps};

/// Contract-fixed display name of the plan format step.
pub const FORMAT_STEP_NAME: &str = "Format";

/// Fixed `Format` step over orchestrator-supplied Mise argv plus env.
///
/// The vector must start with `mise`: plan formatting runs through
/// pinned Mise, never a bare toolchain or ad-hoc installer. The env
/// routes the step at the prepared toolchain (owned homes plus the
/// exact `RUSTUP_TOOLCHAIN` pin), scrubbed since fmt needs no auth.
/// # Errors
pub fn format_step(argv: Vec<String>, env: &BTreeMap<String, String>) -> Result<Step, RenderError> {
    if argv.first().is_none_or(|program| program != "mise") {
        return Err(RenderError::BadCommand("format_without_mise".to_owned()));
    }
    let mut step = steps::shell_step(FORMAT_STEP_NAME, argv, env.clone())?;
    step.role = Some(StepRole::PlanFormat);
    Ok(step)
}

/// Insert `Format` into the plan job between staging and freshness.
///
/// Anchors before the freshness check when present, else before the
/// `plan-v1` step, else before plan publish; without a plan job there is
/// nothing to close over. Re-running never duplicates the step.
/// # Errors
pub fn ensure_plan_format(
    jobs: &mut BTreeMap<String, Job>,
    argv: Vec<String>,
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    let Some(plan) = jobs.get_mut(PLAN_JOB_ID) else {
        return Ok(());
    };
    if let Some(format) = plan
        .steps
        .iter()
        .find(|step| step.role == Some(StepRole::PlanFormat))
    {
        return check_format_shape(format);
    }
    let step = format_step(argv, env)?;
    let at = format_insert_at(plan);
    plan.steps.insert(at, step);
    Ok(())
}

/// Reject present-but-malformed `Format` steps (non-shell payloads).
fn check_format_shape(format: &Step) -> Result<(), RenderError> {
    if matches!(format.kind, StepKind::Shell { .. }) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(
            "format_step_malformed".to_owned(),
        ))
    }
}

/// Insert before freshness, else plan, else publish, else at the end.
fn format_insert_at(plan: &Job) -> usize {
    plan.steps
        .iter()
        .position(|step| step.role == Some(StepRole::CheckGenerated))
        .or_else(|| {
            plan.steps
                .iter()
                .position(|step| step.role == Some(StepRole::PlanProducer))
        })
        .or_else(|| {
            plan.steps
                .iter()
                .position(|step| step.role == Some(StepRole::PublishPlan))
        })
        .unwrap_or(plan.steps.len())
}
