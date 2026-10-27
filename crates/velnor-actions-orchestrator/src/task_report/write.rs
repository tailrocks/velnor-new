// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    CacheLayer, CacheOutcome, CacheResult, ExecuteTaskRef, MatrixEntry, Plan, PlatformBinding,
    PlatformRunnerEnvironment, PlatformUnavailableReason, TaskReport, TaskStatus,
    parse_strict_json, task_report_id_for_task, validate_run_key, validate_task_id,
};

use super::observation::{UNIT_OBSERVATION_ENV, stage_unit_observation_to, write_entry_reports};
use super::single_task_aggregate;
use super::task_report_order;
use super::timing::{elapsed_ms, parse_exit_code, parse_start_ms};
use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::internal_request::resolve_run_key;

pub const REPORT_OP: &str = "write-task-report-v1";
pub(crate) const TASK_ID_ENV: &str = "VELNOR_TASK_ID";
pub(crate) const EXIT_CODE_ENV: &str = "VELNOR_EXIT_CODE";
pub(crate) const DOWNSTREAM_IDS_ENV: &str = "VELNOR_DOWNSTREAM_TASK_IDS";
pub(crate) const START_MS_ENV: &str = "VELNOR_START_MS";

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
    let (entry, digest) = entry_and_digest(&plan, task_id)?;
    if entry.stack_id == "mise" && exit_code == 0 {
        return Err(internal("named_check_requires_qualified_execution"));
    }
    let duration_ms = elapsed_ms(start_ms);
    let task = terminal_task_report(&plan, entry, digest, exit_code, duration_ms)
        .map_err(internal_contract)?;
    let matrix = single_task_aggregate(&plan, entry, &task).map_err(internal_contract)?;
    write_entry_reports(runner_temp, &plan, entry, &task, &matrix)?;
    if let Some(observation) = env::var(UNIT_OBSERVATION_ENV)
        .ok()
        .filter(|value| !value.is_empty())
    {
        stage_unit_observation_to(
            runner_temp,
            &plan,
            entry,
            &task.task_report_id,
            &observation,
        )?;
    }
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

fn plan_path(runner_temp: &Path, run_key: &str) -> PathBuf {
    runner_temp.join("velnor").join(run_key).join("plan.json")
}

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
