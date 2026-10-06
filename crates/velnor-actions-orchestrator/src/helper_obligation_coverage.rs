//! Native outcome coverage preserves previously reported downstream skips.

use std::path::Path;

use velnor_actions_contract::{
    HelperObligationOutcome, MatrixEntry, NotSelectedReason, Plan, TaskStatus,
    canonical_json_bytes, parse_strict_json,
};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::task_report::{entry_and_digest, single_task_aggregate, write_entry_reports};

#[path = "helper_downstream.rs"]
mod downstream_authority;

/// Write terminal coverage and retain only identical, previously bound skips.
pub(crate) fn write(
    plan: &Plan,
    entry: &MatrixEntry,
    digest: &str,
    outcome: HelperObligationOutcome,
    temp: &Path,
) -> Result<(), OrchestratorError> {
    if outcome == HelperObligationOutcome::Skipped {
        return skip(plan, entry, digest, temp);
    }
    let exit_code = match outcome {
        HelperObligationOutcome::Success => 0,
        HelperObligationOutcome::Cancelled => 130,
        HelperObligationOutcome::Failure | HelperObligationOutcome::Skipped => 1,
    };
    let mut task = crate::task_report::terminal_task_report(plan, entry, digest, exit_code, None)
        .map_err(internal_contract)?;
    if outcome == HelperObligationOutcome::Cancelled {
        task.status = TaskStatus::Cancelled;
    }
    task.validate().map_err(internal_contract)?;
    let matrix = single_task_aggregate(plan, entry, &task).map_err(internal_contract)?;
    write_entry_reports(temp, plan, entry, &task, &matrix)
}

/// Validate all requested skips against the same planned job before any write.
pub(crate) fn downstream(
    plan: &Plan,
    source: &MatrixEntry,
    ids: &[String],
    temp: &Path,
) -> Result<usize, OrchestratorError> {
    downstream_authority::check(plan, source, ids)?;
    let mut validated = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for id in ids {
        if id == &source.task_id || !seen.insert(id) {
            return Err(internal("invalid_helper_downstream"));
        }
        if crate::covered_tasks::covered_by_baseline(plan, id) {
            continue;
        }
        let (entry, digest) = entry_and_digest(plan, id)?;
        if entry.job_id != source.job_id {
            return Err(internal("helper_downstream_job_mismatch"));
        }
        validated.push((entry, digest));
    }
    for (entry, digest) in &validated {
        skip(plan, entry, digest, temp)?;
    }
    Ok(validated.len())
}

/// A skip can already exist because its upstream obligation reported first.
fn skip(
    plan: &Plan,
    entry: &MatrixEntry,
    digest: &str,
    temp: &Path,
) -> Result<(), OrchestratorError> {
    let task = crate::decisions::not_selected_report(&crate::decisions::NotSelectedInputs {
        run_key: &plan.run_key,
        event: plan.event,
        trust: plan.trust,
        matrix_id: &entry.id,
        matrix_key: &entry.matrix_key,
        task_id: &entry.task_id,
        task_digest: digest,
        reason: NotSelectedReason::UpstreamFailed,
    })
    .map_err(internal_contract)?;
    let matrix = single_task_aggregate(plan, entry, &task).map_err(internal_contract)?;
    let dir = temp
        .join("velnor")
        .join(&plan.run_key)
        .join(&entry.matrix_key);
    crate::exclusive_write::create_dir_no_symlink(temp, &dir.join("tasks"))?;
    let matrix_path = dir.join("matrix-report.json");
    let task_path = dir
        .join("tasks")
        .join(format!("{}.json", task.task_report_id));
    if matrix_path.symlink_metadata().is_ok() || task_path.symlink_metadata().is_ok() {
        equal_existing(
            &matrix_path,
            &canonical_json_bytes(&matrix).map_err(internal_contract)?,
        )?;
        equal_existing(
            &task_path,
            &canonical_json_bytes(&task).map_err(internal_contract)?,
        )?;
        return Ok(());
    }
    write_entry_reports(temp, plan, entry, &task, &matrix)
}

/// Existing coverage must equal the complete expected report, without shadow keys.
fn equal_existing(path: &Path, expected: &[u8]) -> Result<(), OrchestratorError> {
    let text = crate::retrieve_reports::read_staged_text(path, 1024 * 1024)
        .map_err(|kind| internal(&format!("invalid_existing_helper_skip:{kind}")))?;
    let value = parse_strict_json(&text).map_err(|_| internal("invalid_existing_helper_skip"))?;
    let bytes = canonical_json_bytes(&value).map_err(internal_contract)?;
    if bytes != expected {
        return Err(internal("existing_helper_skip_mismatch"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "helper_obligation_coverage_tests.rs"]
mod tests;
