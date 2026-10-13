//! Event-time typed task-report production.
//! Reports bind task and matrix identities through the staged plan. Named
//! Mise success requires the qualified execution producer and final receipt proof.
//!
//! Failed obligations also report downstream tasks as `upstream_failed`.

use std::env;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    CacheLayer, CacheOutcome, CacheResult, ExecuteTaskRef, MatrixEntry, MatrixReport, Plan,
    PlatformBinding, PlatformRunnerEnvironment, PlatformUnavailableReason, TaskReport, TaskStatus,
    canonical_json_bytes, parse_strict_json, task_report_id_for_task, validate_run_key,
    validate_task_id,
};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::internal_request::resolve_run_key;
#[path = "task_report_timing.rs"]
mod timing;
#[cfg(test)]
use timing::now_ms;
use timing::{elapsed_ms, parse_exit_code, parse_start_ms};

pub(crate) use crate::task_report_aggregate::single_task_aggregate;

#[path = "task_report_order.rs"]
mod task_report_order;

/// Report-production operation tag.
pub const REPORT_OP: &str = "write-task-report-v1";
/// Env key carrying the executed obligation's task ID.
pub(crate) const TASK_ID_ENV: &str = "VELNOR_TASK_ID";
/// Env key carrying the captured obligation exit code.
pub(crate) const EXIT_CODE_ENV: &str = "VELNOR_EXIT_CODE";
/// Env key carrying comma-separated downstream task IDs for skip reports.
pub(crate) const DOWNSTREAM_IDS_ENV: &str = "VELNOR_DOWNSTREAM_TASK_IDS";
/// Env key carrying the wrapper-captured start time (unix millis).
pub(crate) const START_MS_ENV: &str = "VELNOR_START_MS";

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
    let reason = crate::noop_report::noop_reason_present();
    let digest = env::var(crate::noop_report::TASK_DIGEST_ENV).ok();
    let exit_code = parse_exit_code(&exit_raw)?;
    if let Some(request) =
        crate::noop_report::parse_noop_request(reason.as_deref(), digest.as_deref())?
    {
        return crate::noop_report::write_noop_report_to(
            &run_key,
            &task_id,
            exit_code,
            &request,
            Path::new(&runner_temp),
        );
    }
    let downstream_env = env::var(DOWNSTREAM_IDS_ENV).ok();
    let downstream = parse_downstream(downstream_env.as_deref());
    let start_ms = env::var(START_MS_ENV)
        .ok()
        .and_then(|raw| parse_start_ms(&raw));
    write_task_report_to(
        &run_key,
        &task_id,
        exit_code,
        start_ms,
        &downstream,
        Path::new(&runner_temp),
    )
}

/// Write reports for one outcome with explicit inputs (testable core).
///
/// The plan at `$RUNNER_TEMP/velnor/<run-key>/plan.json` binds every
/// identity: task digest, matrix coordinates, event, and trust. A nonzero
/// exit additionally reports each downstream ID as skipped; anything the
/// plan cannot bind errors instead of emitting unbound bytes. A
/// baseline-covered obligation proves nothing here: the merge
/// revalidates it against the manifest, so this op succeeds silently
/// with zero reports instead of failing `task_not_in_plan`.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for invalid plans, run-key
/// mismatches, unknown or multi-task entries, and unwritable paths.
pub(crate) fn write_task_report_to(
    run_key: &str,
    task_id: &str,
    exit_code: i32,
    start_ms: Option<u64>,
    downstream: &[String],
    runner_temp: &Path,
) -> Result<usize, OrchestratorError> {
    validate_run_key(run_key).map_err(internal_contract)?;
    if !(0..=255).contains(&exit_code) {
        return Err(internal("bad_exit_code"));
    }
    let plan = load_plan(run_key, runner_temp)?;
    if crate::covered_tasks::covered_by_baseline(&plan, task_id) {
        return Ok(0);
    }
    let (entry, digest) = entry_and_digest(&plan, task_id)?;
    if entry.stack_id == "mise" && exit_code == 0 {
        return Err(internal("named_check_requires_qualified_execution"));
    }
    let duration_ms = elapsed_ms(start_ms);
    let task = terminal_task_report(&plan, entry, digest, exit_code, duration_ms)
        .map_err(internal_contract)?;
    let matrix = single_task_aggregate(&plan, entry, &task).map_err(internal_contract)?;
    write_entry_reports(runner_temp, &plan, entry, &task, &matrix)?;
    let mut reported = 1usize;
    if exit_code != 0 {
        let downstream_tasks: Vec<String> = if downstream.is_empty() {
            task_report_order::derive_downstream(&plan, task_id, &entry.job_id)
        } else {
            downstream.to_vec()
        };
        reported +=
            crate::noop_report::write_skip_reports(&plan, task_id, &downstream_tasks, runner_temp)?;
    }
    Ok(reported)
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
///
/// Reads through the shared staged-text gate (symlink rejection plus
/// the retrieve-side bound for this same `plan.json`) and the strict
/// duplicate-rejecting parser, so a hostile plan fails closed instead
/// of exhausting memory or smuggling shadowed keys.
pub(crate) fn load_plan(run_key: &str, runner_temp: &Path) -> Result<Plan, OrchestratorError> {
    let path = plan_path(runner_temp, run_key);
    let bound = crate::retrieve_reports::MAX_RETRIEVE_PLAN_BYTES;
    let text = match crate::retrieve_reports::read_staged_text(&path, bound) {
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
pub(crate) fn entry_and_digest<'a>(
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
pub(crate) fn entry_and_digest_for_job<'a>(
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
pub(crate) fn terminal_task_report(
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
        platform_binding: PlatformBinding::unavailable(
            &entry.planned_platform.platform_id,
            PlatformRunnerEnvironment::Unknown,
            PlatformUnavailableReason::ObservationNotRecorded,
        )?,
        exit_code,
        duration_ms,
        outputs: Vec::new(),
        lane: None,
        queue: None,
        partition: None,
        reason: None,
        timing: crate::schedule::measured_timing(duration_ms),
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
pub(crate) fn write_entry_reports(
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
    crate::exclusive_write::create_dir_no_symlink(runner_temp, &task_files_dir)?;
    let matrix_bytes = canonical_json_bytes(matrix).map_err(internal_contract)?;
    let task_bytes = canonical_json_bytes(task).map_err(internal_contract)?;
    crate::exclusive_write::write_exclusive(
        &dir.join("matrix-report.json"),
        &matrix_bytes,
        "report",
    )?;
    crate::exclusive_write::write_exclusive(
        &task_files_dir.join(format!("{}.json", task.task_report_id)),
        &task_bytes,
        "report",
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "task_report_cover_tests.rs"]
mod task_report_cover_tests;
#[cfg(test)]
#[path = "task_report_merge_tests.rs"]
mod task_report_merge_tests;
#[cfg(test)]
#[path = "task_report_order_tests.rs"]
mod task_report_order_tests;
#[cfg(test)]
#[path = "task_report_tests.rs"]
mod task_report_tests;

#[cfg(test)]
#[path = "check_gate_tests.rs"]
mod check_gate_tests;
