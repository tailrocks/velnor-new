//! Staged-plan task-report primitives: bounded loads, entry resolution,
//! terminal derivation, and canonical report writes.
//!
//! Every identity binds through the downloaded plan at
//! `$RUNNER_TEMP/velnor/<run-key>/plan.json`: task digest, matrix
//! coordinates, event, and trust. Anything the plan cannot bind
//! errors instead of emitting unbound bytes.

use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    canonical_json_bytes, parse_strict_json, task_report_id_for_task, validate_task_id,
};
use velnor_actions_contract_workflow::{
    CacheLayer, CacheOutcome, CacheResult, ExecuteTaskRef, MatrixEntry, MatrixReport, Plan,
    TaskReport, TaskStatus,
};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::{internal, internal_contract};

mod task_report_order;
pub use task_report_order::derive_downstream;

pub use super::task_report_aggregate::single_task_aggregate;

/// Load and validate the downloaded plan, bound to this run key.
///
/// Reads through the shared staged-text gate (symlink rejection plus
/// the caller-supplied byte bound for this same `plan.json`) and the
/// strict duplicate-rejecting parser, so a hostile plan fails closed
/// instead of exhausting memory or smuggling shadowed keys. Callers
/// pass the retrieve-side bound so both readers of `plan.json` share
/// one limit.
pub fn load_plan(
    run_key: &str,
    runner_temp: &Path,
    max_plan_bytes: u64,
) -> Result<Plan, OrchestratorError> {
    let path = plan_path(runner_temp, run_key);
    let text = match velnor_actions_orchestrator_core::staged_reads::read_staged_text(
        &path,
        max_plan_bytes,
    ) {
        Ok(text) => text,
        Err("missing") => {
            return Err(OrchestratorError::io(
                path.display().to_string(),
                "not_found",
            ));
        }
        Err(kind) => return Err(internal(&format!("unreadable_plan:{kind}"))),
    };
    let value = parse_strict_json(&text).map_err(|_| internal("unparsable_plan"))?;
    let plan: Plan = serde_json::from_value(value).map_err(|_| internal("unparsable_plan"))?;
    plan.validate().map_err(internal_contract)?;
    if plan.run_key != run_key {
        return Err(internal("report_run_mismatch"));
    }
    Ok(plan)
}

/// Downloaded plan path under runner temp.
fn plan_path(runner_temp: &Path, run_key: &str) -> PathBuf {
    runner_temp.join("velnor").join(run_key).join("plan.json")
}

/// Obligation entry plus plan task digest for one task ID.
///
/// The entry must name exactly this task: multi-task (sharded-ref)
/// entries have no per-task outcome here and refuse rather than emit a
/// lying aggregate. No producer emits sharded refs today.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for unknown tasks, tasks
/// without an obligation digest, and multi-task entries.
pub fn entry_and_digest<'a>(
    plan: &'a Plan,
    task_id: &str,
) -> Result<(&'a MatrixEntry, &'a str), OrchestratorError> {
    validate_task_id(task_id).map_err(internal_contract)?;
    let mut found = None;
    for entry in &plan.matrix.include {
        if !flattened(entry).contains(&task_id) {
            continue;
        }
        if flattened(entry).len() != 1 {
            return Err(internal("multi_task_entry"));
        }
        if found.is_some() {
            return Err(internal("duplicate_task_entry"));
        }
        found = Some(entry);
    }
    let entry = found.ok_or_else(|| internal("task_not_in_plan"))?;
    let digest = plan
        .obligations
        .iter()
        .find(|obligation| obligation.task_id == task_id)
        .map(|obligation| obligation.task_digest.as_str())
        .ok_or_else(|| internal("task_without_obligation"))?;
    Ok((entry, digest))
}

/// Resolve one named-check report entry by its generated job identity.
pub fn entry_and_digest_for_job<'a>(
    plan: &'a Plan,
    task_id: &str,
    job_id: &str,
) -> Result<(&'a MatrixEntry, &'a str), OrchestratorError> {
    validate_task_id(task_id).map_err(internal_contract)?;
    let mut found = None;
    for entry in &plan.matrix.include {
        if entry.job_id != job_id || !flattened(entry).contains(&task_id) {
            continue;
        }
        if flattened(entry).len() != 1 {
            return Err(internal("multi_task_entry"));
        }
        if found.is_some() {
            return Err(internal("duplicate_task_entry"));
        }
        found = Some(entry);
    }
    let entry = found.ok_or_else(|| internal("task_not_in_plan_for_job"))?;
    let digest = plan
        .obligations
        .iter()
        .find(|obligation| obligation.task_id == task_id)
        .map(|obligation| obligation.task_digest.as_str())
        .ok_or_else(|| internal("task_without_obligation"))?;
    Ok((entry, digest))
}

/// Declared task IDs of one matrix entry, sorted.
fn flattened(entry: &MatrixEntry) -> Vec<&str> {
    let mut ids = Vec::new();
    for task_ref in entry.execute_task_ids.tasks.values() {
        match task_ref {
            ExecuteTaskRef::Single(id) => ids.push(id.as_str()),
            ExecuteTaskRef::Shards(shards) => ids.extend(shards.iter().map(String::as_str)),
        }
    }
    ids.sort_unstable();
    ids
}

/// Validated terminal report: `executed` on exit 0, else `failed`.
///
/// # Errors
///
/// Returns [`ContractError`] for derivation or validation failures.
pub fn terminal_task_report(
    plan: &Plan,
    entry: &MatrixEntry,
    task_digest: &str,
    exit_code: i32,
    duration_ms: Option<u64>,
) -> Result<TaskReport, velnor_actions_contract::ContractError> {
    let status = if exit_code == 0 {
        TaskStatus::Executed
    } else {
        TaskStatus::Failed
    };
    let report = TaskReport {
        schema: TaskReport::SCHEMA,
        task_report_id: task_report_id_for_task(&plan.run_key, &entry.matrix_key, task_digest)?,
        run_key: plan.run_key.clone(),
        event: plan.event,
        trust: plan.trust,
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        task_id: entry.task_id.clone(),
        task_digest: task_digest.to_owned(),
        status,
        not_selected_reason: None,
        cache: CacheOutcome {
            layer: CacheLayer::Task,
            key: String::new(),
            result: CacheResult::NotAttempted,
            miss_reason: None,
        },
        exit_code,
        duration_ms,
        outputs: Vec::new(),
        lane: None,
        queue: None,
        partition: None,
        reason: None,
        timing: velnor_actions_orchestrator_core::schedule::measured_timing(duration_ms),
    };
    report.validate()?;
    report.validate_outputs_declared(&entry.declared_outputs)?;
    Ok(report)
}

/// Write one entry's matrix report plus its task file as canonical JSON.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for encoding failures and
/// [`OrchestratorError::Io`] for unwritable directories; pre-existing
/// files error, never overwrite.
pub fn write_entry_reports(
    runner_temp: &Path,
    plan: &Plan,
    entry: &MatrixEntry,
    task: &TaskReport,
    matrix: &MatrixReport,
) -> Result<(), OrchestratorError> {
    let dir = runner_temp
        .join("velnor")
        .join(&plan.run_key)
        .join(&entry.matrix_key);
    let task_files_dir = dir.join("tasks");
    velnor_actions_orchestrator_core::exclusive_write::create_dir_no_symlink(
        runner_temp,
        &task_files_dir,
    )?;
    let matrix_bytes = canonical_json_bytes(matrix).map_err(internal_contract)?;
    let task_bytes = canonical_json_bytes(task).map_err(internal_contract)?;
    velnor_actions_orchestrator_core::exclusive_write::write_exclusive(
        &dir.join("matrix-report.json"),
        &matrix_bytes,
        "report",
    )?;
    velnor_actions_orchestrator_core::exclusive_write::write_exclusive(
        &task_files_dir.join(format!("{}.json", task.task_report_id)),
        &task_bytes,
        "report",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
