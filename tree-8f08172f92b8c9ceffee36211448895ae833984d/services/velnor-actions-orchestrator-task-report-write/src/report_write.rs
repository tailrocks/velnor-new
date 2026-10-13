//! Event-time typed task-report production.
//!
//! Reports bind task and matrix identities through the staged plan. Named
//! Mise success requires the qualified execution producer and final receipt proof.
//!
//! Failed obligations also report downstream tasks as `upstream_failed`.

use std::env;
use std::path::Path;

use velnor_actions_contract::validate_run_key;

use super::timing::{elapsed_ms, parse_exit_code, parse_start_ms};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::{internal, internal_contract};
use velnor_actions_orchestrator_task_report::task_report::{
    TaskRuntimeIdentity, TaskRuntimeReceipt, derive_downstream, entry_and_digest, load_plan,
    single_task_aggregate, terminal_task_report, write_entry_reports_with_runtime_receipt,
};

use velnor_actions_orchestrator_core::report_keys::{
    DOWNSTREAM_IDS_ENV, EXIT_CODE_ENV, START_MS_ENV, TASK_ID_ENV,
};

/// Maximum bytes read for the report-step plan.
///
/// Matches the retrieve-step plan bound: the same `plan.json` parses
/// identically at retrieve and report time, and a giant plan errors
/// instead of exhausting the job's memory.
const MAX_RETRIEVE_PLAN_BYTES: u64 = 4 << 20;

/// Write the executed obligation's reports plus downstream skip reports.
///
/// Takes the hub-resolved run key and the plan from the downloaded plan
/// artifact; returns the count of tasks reported (one plus downstream
/// skips on failure).
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for missing env, unreadable or
/// invalid plans, run-key mismatches, unknown or multi-task entries, and
/// unwritable report paths; [`OrchestratorError::Io`] for IO failures.
pub fn write_task_report_with_key(run_key: &str) -> Result<usize, OrchestratorError> {
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
    let reason = velnor_actions_orchestrator_noop_report::noop_report::noop_reason_present();
    let digest =
        env::var(velnor_actions_orchestrator_noop_report::noop_report::TASK_DIGEST_ENV).ok();
    let exit_code = parse_exit_code(&exit_raw)?;
    if let Some(request) = velnor_actions_orchestrator_noop_report::noop_report::parse_noop_request(
        reason.as_deref(),
        digest.as_deref(),
    )? {
        return velnor_actions_orchestrator_noop_report::noop_report::write_noop_report_to(
            run_key,
            &task_id,
            exit_code,
            &request,
            Path::new(&runner_temp),
            MAX_RETRIEVE_PLAN_BYTES,
        );
    }
    let runtime = runtime_identity_for_run_key(run_key, |name| env::var(name).ok())?;
    let downstream_env = env::var(DOWNSTREAM_IDS_ENV).ok();
    let downstream = parse_downstream(downstream_env.as_deref());
    let start_ms = env::var(START_MS_ENV)
        .ok()
        .and_then(|raw| parse_start_ms(&raw));
    write_task_report_to_with_runtime(
        run_key,
        &task_id,
        exit_code,
        start_ms,
        &downstream,
        runtime.as_ref(),
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
pub fn write_task_report_to(
    run_key: &str,
    task_id: &str,
    exit_code: i32,
    start_ms: Option<u64>,
    downstream: &[String],
    runner_temp: &Path,
) -> Result<usize, OrchestratorError> {
    write_task_report_to_with_runtime(
        run_key,
        task_id,
        exit_code,
        start_ms,
        downstream,
        None,
        runner_temp,
    )
}

/// Write reports with an explicitly supplied GitHub runtime identity.
///
/// Local callers can keep using [`write_task_report_to`]; the environment
/// entrypoint supplies identity only for a GitHub run and writes a separate
/// versioned receipt beside the schema-1 reports.
///
/// # Errors
/// Returns [`OrchestratorError::Internal`] for invalid identities or plans,
/// and IO errors for staged reads and report writes.
pub fn write_task_report_to_with_runtime(
    run_key: &str,
    task_id: &str,
    exit_code: i32,
    start_ms: Option<u64>,
    downstream: &[String],
    runtime: Option<&TaskRuntimeIdentity>,
    runner_temp: &Path,
) -> Result<usize, OrchestratorError> {
    validate_run_key(run_key).map_err(internal_contract)?;
    if !(0..=255).contains(&exit_code) {
        return Err(internal("bad_exit_code"));
    }
    let plan = load_plan(run_key, runner_temp, MAX_RETRIEVE_PLAN_BYTES)?;
    if velnor_actions_orchestrator_covered_tasks::covered_tasks::covered_by_baseline(&plan, task_id)
    {
        return Ok(0);
    }
    let (entry, digest) = entry_and_digest(&plan, task_id)?;
    if entry.stack_id == "mise" && exit_code == 0 {
        return Err(internal("named_check_requires_qualified_execution"));
    }
    let duration_ms = elapsed_ms(start_ms);
    let task = terminal_task_report(&plan, entry, digest, exit_code, duration_ms)
        .map_err(internal_contract)?;
    let receipt = runtime
        .map(|identity| TaskRuntimeReceipt::derive(&plan, entry, &task.task_report_id, identity))
        .transpose()
        .map_err(internal_contract)?;
    let matrix = single_task_aggregate(&plan, entry, &task).map_err(internal_contract)?;
    write_entry_reports_with_runtime_receipt(
        runner_temp,
        &plan,
        entry,
        &task,
        &matrix,
        receipt.as_ref(),
    )?;
    let mut reported = 1usize;
    if exit_code != 0 {
        let downstream_tasks: Vec<String> = if downstream.is_empty() {
            derive_downstream(&plan, task_id, &entry.job_id)
        } else {
            downstream.to_vec()
        };
        reported += velnor_actions_orchestrator_noop_report::noop_report::write_skip_reports(
            &plan,
            task_id,
            &downstream_tasks,
            runner_temp,
        )?;
    }
    Ok(reported)
}

fn runtime_identity_for_run_key(
    run_key: &str,
    mut read: impl FnMut(&str) -> Option<String>,
) -> Result<Option<TaskRuntimeIdentity>, OrchestratorError> {
    if run_key == "local" {
        return Ok(None);
    }
    let required = |name: &str, read: &mut dyn FnMut(&str) -> Option<String>| {
        read(name)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| internal(&format!("missing_runtime_identity:{name}")))
    };
    let repository = required("GITHUB_REPOSITORY", &mut read)?;
    let run_id = required("GITHUB_RUN_ID", &mut read)?;
    let attempt = required("GITHUB_RUN_ATTEMPT", &mut read)?;
    let run_attempt = attempt
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0 && value.to_string() == attempt)
        .ok_or_else(|| internal("bad_runtime_run_attempt"))?;
    let source_sha = required("GITHUB_SHA", &mut read)?;
    let workflow_ref = required("GITHUB_WORKFLOW_REF", &mut read)?;
    let workflow_job_key = required("GITHUB_JOB", &mut read)?;
    let runner_name = required("RUNNER_NAME", &mut read)?;
    let identity = TaskRuntimeIdentity::new(
        repository,
        run_id.clone(),
        run_attempt,
        source_sha,
        workflow_ref,
        workflow_job_key,
        runner_name,
    )
    .map_err(internal_contract)?;
    if run_key != format!("r{run_id}-a{run_attempt}") {
        return Err(internal("runtime_run_key_mismatch"));
    }
    Ok(Some(identity))
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

#[cfg(test)]
mod tests;
