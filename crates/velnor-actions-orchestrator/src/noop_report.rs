use std::path::Path;

use velnor_actions_contract::{NotSelectedReason, Plan, validate_run_key};

use crate::task_report::{entry_and_digest, single_task_aggregate, write_entry_reports};

use crate::OrchestratorError;
use crate::decisions::{NotSelectedInputs, not_selected_report};
use crate::internal::{internal, internal_contract};

pub(crate) const NOT_SELECTED_REASON_ENV: &str = "VELNOR_NOT_SELECTED_REASON";
pub(crate) const TASK_DIGEST_ENV: &str = "VELNOR_NOOP_TASK_DIGEST";

pub(crate) struct NoOpRequest {
    pub(crate) reason: NotSelectedReason,
    pub(crate) task_digest: String,
}

pub(crate) fn parse_not_selected_reason(raw: &str) -> Result<NotSelectedReason, OrchestratorError> {
    match raw {
        "upstream_failed" => Ok(NotSelectedReason::UpstreamFailed),
        "not_in_plan" => Ok(NotSelectedReason::NotInPlan),
        "unsupported" => Ok(NotSelectedReason::Unsupported),
        "cancelled_by_policy" => Ok(NotSelectedReason::CancelledByPolicy),
        _ => Err(internal("bad_not_selected_reason")),
    }
}

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

pub(crate) fn noop_reason_present() -> Option<String> {
    std::env::var(NOT_SELECTED_REASON_ENV).ok()
}

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
    let (entry, digest) = entry_and_digest(&plan, task_id)?;
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
        let (entry, digest) = entry_and_digest(plan, downstream_id)?;
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
        write_entry_reports(runner_temp, plan, entry, &task, &matrix)?;
        reported += 1;
    }
    Ok(reported)
}
