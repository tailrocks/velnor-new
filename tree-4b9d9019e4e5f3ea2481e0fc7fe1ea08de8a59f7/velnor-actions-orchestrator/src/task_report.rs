//! Event-time `write-task-report-v1`: typed crate-job report production.
//!
//! P05 grouped obligations into crate jobs but left no producer behind:
//! legs-era reporting died with the matrix template, so hosted merges see
//! zero artifacts and fail closed on `no_entry`/`source_missing`. This op
//! closes the gap. Each obligation wrapper captures its exit code and
//! invokes the staged helper here; the op resolves the obligation against
//! the downloaded plan (digests stay plan-bound, never generator-baked)
//! and writes the validated `TaskReport` plus its entry's single-task
//! `MatrixReport` through the contract canonical JSON — report bytes are
//! produced by Rust, never shell-composed.
//!
//! A failing obligation also reports every downstream same-crate
//! obligation as `not_selected`/`upstream_failed` (task-execution
//! contract: skipped obligations still report), because GitHub skips the
//! later steps and nothing else could speak for them.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    CacheLayer, CacheOutcome, CacheResult, ExecuteTaskRef, MatrixEntry, MatrixReport, MatrixStatus,
    MatrixTaskEntry, NotSelectedReason, Plan, TaskReport, TaskStatus, canonical_json_bytes,
    report_id_for_matrix, task_report_id_for_task, validate_run_key, validate_task_id,
};

use crate::OrchestratorError;
use crate::decisions::not_selected_report;
use crate::internal::{internal, internal_contract};
use crate::internal_request::resolve_run_key;

/// Report-production operation tag.
pub const REPORT_OP: &str = "write-task-report-v1";
/// Env key carrying the executed obligation's task ID.
pub(crate) const TASK_ID_ENV: &str = "VELNOR_TASK_ID";
/// Env key carrying the captured obligation exit code.
pub(crate) const EXIT_CODE_ENV: &str = "VELNOR_EXIT_CODE";
/// Env key carrying comma-separated downstream task IDs for skip reports.
pub(crate) const DOWNSTREAM_IDS_ENV: &str = "VELNOR_DOWNSTREAM_TASK_IDS";

/// Write the executed obligation's reports plus downstream skip reports.
///
/// Resolves the run key from the GitHub environment and the plan from the
/// downloaded plan artifact; returns the count of tasks reported (one plus
/// downstream skips on failure).
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for missing env, unreadable or
/// invalid plans, run-key mismatches, unknown or multi-task entries, and
/// unwritable report paths; [`OrchestratorError::Io`] for IO failures.
pub fn write_task_report() -> Result<usize, OrchestratorError> {
    let run_key = resolve_run_key(None)?;
    let runner_temp = env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let task_id = env::var(TASK_ID_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_task_id"))?;
    let exit_raw = env::var(EXIT_CODE_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_exit_code"))?;
    let downstream_env = env::var(DOWNSTREAM_IDS_ENV).ok();
    let downstream = parse_downstream(downstream_env.as_deref());
    write_task_report_to(
        &run_key,
        &task_id,
        parse_exit_code(&exit_raw)?,
        &downstream,
        Path::new(&runner_temp),
    )
}

/// Write reports for one outcome with explicit inputs (testable core).
///
/// The plan at `$RUNNER_TEMP/velnor/<run-key>/plan.json` binds every
/// identity: task digest, matrix coordinates, event, and trust. A nonzero
/// exit additionally reports each downstream ID as skipped; anything the
/// plan cannot bind errors instead of emitting unbound bytes.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for invalid plans, run-key
/// mismatches, unknown or multi-task entries, and unwritable paths.
pub(crate) fn write_task_report_to(
    run_key: &str,
    task_id: &str,
    exit_code: i32,
    downstream: &[String],
    runner_temp: &Path,
) -> Result<usize, OrchestratorError> {
    validate_run_key(run_key).map_err(internal_contract)?;
    if !(0..=255).contains(&exit_code) {
        return Err(internal("bad_exit_code"));
    }
    let plan = load_plan(run_key, runner_temp)?;
    let (entry, digest) = entry_and_digest(&plan, task_id)?;
    let task = terminal_task_report(&plan, entry, digest, exit_code).map_err(internal_contract)?;
    let matrix = single_task_aggregate(&plan, entry, &task).map_err(internal_contract)?;
    write_entry_reports(runner_temp, &plan, entry, &task, &matrix)?;
    let mut reported = 1usize;
    if exit_code != 0 {
        reported += write_skip_reports(&plan, task_id, downstream, runner_temp)?;
    }
    Ok(reported)
}

/// Parse a captured `$?` value: an integer in the 8-bit exit range.
fn parse_exit_code(raw: &str) -> Result<i32, OrchestratorError> {
    raw.parse::<i32>()
        .ok()
        .filter(|code| (0..=255).contains(code))
        .ok_or_else(|| internal("bad_exit_code"))
}

/// Split downstream IDs on commas, dropping blanks and duplicates.
fn parse_downstream(raw: Option<&str>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut ids = Vec::new();
    for id in raw.unwrap_or_default().split(',') {
        let id = id.trim();
        if !id.is_empty() && seen.insert(id.to_owned()) {
            ids.push(id.to_owned());
        }
    }
    ids
}

/// Load and validate the downloaded plan, bound to this run key.
fn load_plan(run_key: &str, runner_temp: &Path) -> Result<Plan, OrchestratorError> {
    let path = plan_path(runner_temp, run_key);
    let text = fs::read_to_string(&path)
        .map_err(|err| OrchestratorError::io(path.display().to_string(), err.to_string()))?;
    let plan: Plan = serde_json::from_str(&text).map_err(|_| internal("unparsable_plan"))?;
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
fn entry_and_digest<'a>(
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
fn terminal_task_report(
    plan: &Plan,
    entry: &MatrixEntry,
    task_digest: &str,
    exit_code: i32,
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
        duration_ms: 0,
        outputs: Vec::new(),
        lane: None,
        queue: None,
        partition: None,
        reason: None,
        timing: None,
    };
    report.validate()?;
    report.validate_outputs_declared(&entry.declared_outputs)?;
    Ok(report)
}

/// Validated single-task aggregate over one written task report.
///
/// # Errors
///
/// Returns [`ContractError`] for derivation or validation failures.
fn single_task_aggregate(
    plan: &Plan,
    entry: &MatrixEntry,
    task: &TaskReport,
) -> Result<MatrixReport, velnor_actions_contract::ContractError> {
    let mut aggregate = MatrixReport {
        schema: MatrixReport::SCHEMA,
        report_id: report_id_for_matrix(&plan.run_key, &entry.matrix_key)?,
        run_key: plan.run_key.clone(),
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        status: MatrixStatus::Passed,
        expected_task_ids: vec![task.task_id.clone()],
        task_report_ids: vec![task.task_report_id.clone()],
        tasks: vec![MatrixTaskEntry {
            task_report_id: task.task_report_id.clone(),
            task_id: task.task_id.clone(),
            status: task.status,
            exit_code: task.exit_code,
        }],
        selected: 1,
        reused: 0,
        executed: 0,
        empty_partition: 0,
        not_selected: 0,
        failed: 0,
        cancelled: 0,
    };
    match task.status {
        TaskStatus::Reused => aggregate.reused = 1,
        TaskStatus::Executed => aggregate.executed = 1,
        TaskStatus::EmptyPartition => aggregate.empty_partition = 1,
        TaskStatus::NotSelected => aggregate.not_selected = 1,
        TaskStatus::Failed => {
            aggregate.failed = 1;
            aggregate.status = MatrixStatus::Failed;
        }
        TaskStatus::Cancelled => {
            aggregate.cancelled = 1;
            aggregate.status = MatrixStatus::Cancelled;
        }
    }
    if aggregate.report_id != entry.report_id {
        return Err(velnor_actions_contract::ContractError::identity(
            "report_id",
            "report_mismatch",
        ));
    }
    aggregate.validate()?;
    Ok(aggregate)
}

/// Write one entry's matrix report plus its task file as canonical JSON.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for encoding failures and
/// [`OrchestratorError::Io`] for unwritable directories; pre-existing
/// files error, never overwrite.
fn write_entry_reports(
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
    fs::create_dir_all(&task_files_dir).map_err(|err| {
        OrchestratorError::io(task_files_dir.display().to_string(), err.to_string())
    })?;
    let matrix_bytes = canonical_json_bytes(matrix).map_err(internal_contract)?;
    let task_bytes = canonical_json_bytes(task).map_err(internal_contract)?;
    write_new(&dir.join("matrix-report.json"), &matrix_bytes)?;
    write_new(
        &task_files_dir.join(format!("{}.json", task.task_report_id)),
        &task_bytes,
    )?;
    Ok(())
}

/// Exclusively write one report file; a pre-existing file errors.
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), OrchestratorError> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| internal("report_exists"))
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(bytes)
                .map_err(|_| internal("report_unwritable"))
        })
}

/// Report every downstream ID as skipped behind a failure.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for unbound downstream IDs
/// and unwritable paths.
fn write_skip_reports(
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
        let task = not_selected_report(&crate::decisions::NotSelectedInputs {
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
#[path = "task_report_tests.rs"]
mod task_report_tests;
