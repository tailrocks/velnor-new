//! Event-time no-op reports through `write-task-report-v1`.
//!
//! Matrix legs skip inapplicable named steps, but every skipped
//! obligation still reports: the no-op step invokes the staged helper
//! with [`NOT_SELECTED_REASON_ENV`] set, and this variant resolves the
//! obligation against the downloaded plan (digests stay plan-bound)
//! and writes the validated `not_selected` report plus its single-task
//! aggregate through the contract canonical JSON — report bytes are
//! produced by Rust, never shell-composed.

use std::path::Path;

use velnor_actions_contract::validate_run_key;
use velnor_actions_contract_workflow::{NotSelectedReason, Plan};

use crate::task_report::{entry_and_digest, single_task_aggregate, write_entry_reports};

use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::decisions::{NotSelectedInputs, not_selected_report};
use velnor_actions_orchestrator_core::{internal, internal_contract};

/// Env key carrying the no-op `not_selected` reason vocabulary word.
pub(crate) const NOT_SELECTED_REASON_ENV: &str = "VELNOR_NOT_SELECTED_REASON";
/// Env key carrying the skipped obligation's expected task digest.
///
/// Must equal the renderer's `NOOP_DIGEST_ENV` and must differ from the
/// exec leg digest key: otherwise every executed obligation dispatches
/// into the no-op half-present error.
pub(crate) const TASK_DIGEST_ENV: &str = "VELNOR_NOOP_TASK_DIGEST";

/// One validated no-op report request.
pub(crate) struct NoOpRequest {
    /// Closed-vocabulary skip reason.
    pub(crate) reason: NotSelectedReason,
    /// Expected task digest, cross-checked against the plan.
    pub(crate) task_digest: String,
}

/// Parse one closed-vocabulary `not_selected` reason.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for unknown reasons.
pub(crate) fn parse_not_selected_reason(raw: &str) -> Result<NotSelectedReason, OrchestratorError> {
    match raw {
        "upstream_failed" => Ok(NotSelectedReason::UpstreamFailed),
        "not_in_plan" => Ok(NotSelectedReason::NotInPlan),
        "unsupported" => Ok(NotSelectedReason::Unsupported),
        "cancelled_by_policy" => Ok(NotSelectedReason::CancelledByPolicy),
        _ => Err(internal("bad_not_selected_reason")),
    }
}

/// Parse the no-op request env, when the step carries a reason.
///
/// A reason without the expected digest (or vice versa) fails closed:
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for unknown reasons,
/// malformed digests, and half-present pairs.
pub(crate) fn parse_noop_request(
    reason: Option<&str>,
    task_digest: Option<&str>,
) -> Result<Option<NoOpRequest>, OrchestratorError> {
    match (reason, task_digest) {
        (None, None) => Ok(None),
        (Some(reason), Some(task_digest)) => {
            velnor_actions_contract::validate_digest(task_digest).map_err(internal_contract)?;
            Ok(Some(NoOpRequest {
                reason: parse_not_selected_reason(reason)?,
                task_digest: task_digest.to_owned(),
            }))
        }
        _ => Err(internal("noop_half_present")),
    }
}

/// Whether the current op invocation selects the no-op variant.
pub(crate) fn noop_reason_present() -> Option<String> {
    std::env::var(NOT_SELECTED_REASON_ENV).ok()
}

/// Write one no-op `not_selected` report with explicit inputs.
///
/// The exit code must be zero (a reason plus a failure is
/// contradictory) and the expected digest must match the plan binding;
/// anything else errors instead of emitting unbound bytes. A
/// baseline-covered obligation succeeds silently with zero reports:
/// the merge revalidates it against the manifest.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed run keys,
/// contradictory exits, digest mismatches, and the plan-resolution
/// failures of the base op.
pub(crate) fn write_noop_report_to(
    run_key: &str,
    task_id: &str,
    exit_code: i32,
    request: &NoOpRequest,
    runner_temp: &Path,
) -> Result<usize, OrchestratorError> {
    validate_run_key(run_key).map_err(internal_contract)?;
    if exit_code != 0 {
        return Err(internal("reason_with_failure"));
    }
    let plan = crate::task_report::load_plan(run_key, runner_temp)?;
    if velnor_actions_orchestrator_covered_tasks::covered_tasks::covered_by_baseline(&plan, task_id)
    {
        return Ok(0);
    }
    let (entry, digest) = entry_and_digest(&plan, task_id)?;
    if digest != request.task_digest {
        return Err(internal("noop_digest_mismatch"));
    }
    let task = not_selected_report(&NotSelectedInputs {
        run_key: &plan.run_key,
        event: plan.event,
        trust: plan.trust,
        matrix_id: &entry.id,
        matrix_key: &entry.matrix_key,
        task_id,
        task_digest: digest,
        reason: request.reason,
    })
    .map_err(internal_contract)?;
    let matrix = single_task_aggregate(&plan, entry, &task).map_err(internal_contract)?;
    write_entry_reports(runner_temp, &plan, entry, &task, &matrix)?;
    Ok(1)
}

/// Report every downstream ID as skipped behind a failure.
///
/// Baseline-covered downstream IDs prove nothing here: the merge
/// revalidates them against the manifest, so they pass through
/// silently instead of failing `task_not_in_plan` and hiding the
/// later skips behind them.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for unbound downstream IDs
/// and unwritable paths.
pub(crate) fn write_skip_reports(
    plan: &Plan,
    task_id: &str,
    downstream: &[String],
    runner_temp: &Path,
) -> Result<usize, OrchestratorError> {
    let mut reported = 0usize;
    for downstream_id in downstream {
        if downstream_id == task_id {
            return Err(internal("downstream_self"));
        }
        if velnor_actions_orchestrator_covered_tasks::covered_tasks::covered_by_baseline(
            plan,
            downstream_id,
        ) {
            continue;
        }
        let (entry, digest) = entry_and_digest(plan, downstream_id)?;
        let task = not_selected_report(&NotSelectedInputs {
            run_key: &plan.run_key,
            event: plan.event,
            trust: plan.trust,
            matrix_id: &entry.id,
            matrix_key: &entry.matrix_key,
            task_id: downstream_id,
            task_digest: digest,
            reason: NotSelectedReason::UpstreamFailed,
        })
        .map_err(internal_contract)?;
        let matrix = single_task_aggregate(plan, entry, &task).map_err(internal_contract)?;
        write_entry_reports(runner_temp, plan, entry, &task, &matrix)?;
        reported += 1;
    }
    Ok(reported)
}

#[cfg(test)]
mod tests;
