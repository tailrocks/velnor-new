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

use velnor_actions_contract::{NotSelectedReason, Plan, validate_run_key};

use crate::task_report::{entry_and_digest_for_job, single_task_aggregate, write_entry_reports};

use crate::OrchestratorError;
use crate::decisions::{NotSelectedInputs, not_selected_report};
use crate::internal::{internal, internal_contract};

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
/// anything else errors instead of emitting unbound bytes.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed run keys,
/// contradictory exits, digest mismatches, and the plan-resolution
/// failures of the base op.
#[cfg(test)]
pub(crate) fn write_noop_report_to(
    run_key: &str,
    task_id: &str,
    exit_code: i32,
    request: &NoOpRequest,
    runner_temp: &Path,
) -> Result<usize, OrchestratorError> {
    validate_run_key(run_key).map_err(internal_contract)?;
    let plan = crate::task_report::load_plan(run_key, runner_temp)?;
    let mut matching_jobs = plan
        .matrix
        .include
        .iter()
        .filter(|entry| crate::task_report::flattened(entry).contains(&task_id))
        .map(|entry| entry.job_id.as_str());
    let job_id = matching_jobs
        .next()
        .ok_or_else(|| internal("task_not_in_plan"))?;
    if matching_jobs.next().is_some() {
        return Err(internal("ambiguous_task_entry"));
    }
    write_noop_report_for_job(run_key, task_id, exit_code, request, job_id, runner_temp)
}

/// Write one no-op report for the exact GitHub job that selected it.
pub(crate) fn write_noop_report_for_job(
    run_key: &str,
    task_id: &str,
    exit_code: i32,
    request: &NoOpRequest,
    job_id: &str,
    runner_temp: &Path,
) -> Result<usize, OrchestratorError> {
    validate_run_key(run_key).map_err(internal_contract)?;
    if exit_code != 0 {
        return Err(internal("reason_with_failure"));
    }
    let plan = crate::task_report::load_plan(run_key, runner_temp)?;
    let (entry, digest) = entry_and_digest_for_job(&plan, task_id, job_id)?;
    if digest != request.task_digest {
        return Err(internal("noop_digest_mismatch"));
    }
    let task = not_selected_report(&NotSelectedInputs {
        run_key: &plan.run_key,
        event: plan.event,
        trust: plan.trust,
        matrix_id: &entry.id,
        planned_platform: &entry.planned_platform,
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

/// Write plan-bound skip reports only within the failed task's exact job.
///
/// Resolve and build every selected report before the first downstream write.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for self, duplicate, or unbound
/// downstream IDs and [`OrchestratorError::Io`] for unwritable report paths.
pub(crate) fn write_skip_reports_for_job(
    plan: &Plan,
    failed_task_id: &str,
    job_id: &str,
    downstream: &[String],
    runner_temp: &Path,
) -> Result<usize, OrchestratorError> {
    let mut seen = std::collections::BTreeSet::new();
    let mut reports = Vec::with_capacity(downstream.len());
    for downstream_id in downstream {
        if downstream_id == failed_task_id {
            return Err(internal("downstream_self"));
        }
        if !seen.insert(downstream_id.as_str()) {
            return Err(internal("downstream_duplicate"));
        }
        let (entry, digest) = entry_and_digest_for_job(plan, downstream_id, job_id)?;
        let task = not_selected_report(&NotSelectedInputs {
            run_key: &plan.run_key,
            event: plan.event,
            trust: plan.trust,
            matrix_id: &entry.id,
            planned_platform: &entry.planned_platform,
            matrix_key: &entry.matrix_key,
            task_id: downstream_id,
            task_digest: digest,
            reason: NotSelectedReason::UpstreamFailed,
        })
        .map_err(internal_contract)?;
        let matrix = single_task_aggregate(plan, entry, &task).map_err(internal_contract)?;
        reports.push((entry, task, matrix));
    }
    for (entry, task, matrix) in &reports {
        write_entry_reports(runner_temp, plan, entry, task, matrix)?;
    }
    Ok(reports.len())
}

#[cfg(test)]
#[path = "noop_report_tests.rs"]
mod noop_report_tests;
