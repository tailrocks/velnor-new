// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use serde_json::{Value, json};
use velnor_actions_contract::{
    CacheLayer, CacheOutcome, CacheResult, FinalReport, FinalStatus, JobConclusion, JobDisposition,
    MatrixEntry, MatrixReport, MatrixStatus, MatrixTaskEntry, ObligationDecision, Plan,
    PlannedPlatform, TaskReport, TaskStatus, Trust, WorkflowEvent, task_report_id_for_task,
    trust_for_event,
};
use velnor_actions_orchestrator::{MergeContext, merge_internal_with_context};

use super::RUN_KEY;
use super::fixtures_ledger::{branch_plan, group_entry, group_obligation};
use super::fixtures_platform::{digests, observed_binding, planned_platform};
use super::scheduling::fail;

pub(super) fn task_report(
    task: &str,
    digest: &str,
    entry: &MatrixEntry,
    planned: &PlannedPlatform,
) -> Result<TaskReport, Box<dyn Error>> {
    Ok(TaskReport {
        schema: TaskReport::SCHEMA,
        task_report_id: task_report_id_for_task(RUN_KEY, &entry.matrix_key, digest)?,
        run_key: RUN_KEY.to_owned(),
        event: WorkflowEvent::PullRequest,
        trust: Trust::Pr,
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        task_id: task.to_owned(),
        task_digest: digest.to_owned(),
        status: TaskStatus::Executed,
        not_selected_reason: None,
        cache: CacheOutcome {
            layer: CacheLayer::Task,
            key: String::new(),
            result: CacheResult::NotAttempted,
            miss_reason: None,
        },
        platform_binding: observed_binding(planned)?,
        exit_code: 0,
        duration_ms: None,
        outputs: Vec::new(),
        lane: None,
        queue: None,
        partition: None,
        reason: None,
        timing: None,
    })
}

pub(super) fn push_report_value(report: &TaskReport) -> Result<Value, Box<dyn Error>> {
    let mut pushed = report.clone();
    pushed.event = WorkflowEvent::Push;
    pushed.trust = trust_for_event(WorkflowEvent::Push);
    pushed.validate()?;
    Ok(serde_json::to_value(&pushed)?)
}

pub(super) fn matrix_report(task: &str, report: &TaskReport, entry: &MatrixEntry) -> MatrixReport {
    MatrixReport {
        schema: MatrixReport::SCHEMA,
        report_id: entry.report_id.clone(),
        run_key: RUN_KEY.to_owned(),
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        status: MatrixStatus::Passed,
        expected_task_ids: vec![task.to_owned()],
        task_report_ids: vec![report.task_report_id.clone()],
        tasks: vec![MatrixTaskEntry {
            task_report_id: report.task_report_id.clone(),
            task_id: task.to_owned(),
            status: TaskStatus::Executed,
            exit_code: 0,
        }],
        selected: 1,
        reused: 0,
        executed: 1,
        empty_partition: 0,
        not_selected: 0,
        failed: 0,
        cancelled: 0,
    }
}

pub(super) fn branch_typed(
    task: &str,
) -> Result<(Plan, Value, TaskReport, MatrixEntry), Box<dyn Error>> {
    let (digest, input_digest, closure_digest) = digests();
    let planned = planned_platform()?;
    let entry = group_entry(task, &digest, &input_digest, &planned)?;
    let plan = branch_plan(
        task,
        entry.clone(),
        group_obligation(
            task,
            &digest,
            &input_digest,
            &closure_digest,
            ObligationDecision::Execute,
        ),
        JobDisposition::Run,
    )?;
    plan.validate()?;
    let report = task_report(task, &digest, &entry, &planned)?;
    report.validate()?;
    let matrix = matrix_report(task, &report, &entry);
    matrix.validate()?;
    Ok((plan, serde_json::to_value(&matrix)?, report, entry))
}

pub(super) fn required_jobs(conclusions: &[(&str, JobConclusion)]) -> Value {
    conclusions
        .iter()
        .map(|(id, conclusion)| {
            json!({
                "job_id": id,
                "conclusion": conclusion.as_str(),
            })
        })
        .collect()
}

pub(super) fn merge_with_event(
    plan: &Plan,
    matrix_reports: &[Value],
    task_reports: &[Value],
    job_ids: &[&str],
    jobs: &Value,
    ledgers: Option<&[Value]>,
    event: &str,
) -> Result<FinalReport, Box<dyn Error>> {
    let mut request = json!({
        "schema": 1,
        "run_key": RUN_KEY,
        "actual_event": event,
        "plan": plan,
        "matrix": plan.matrix,
        "matrix_reports": matrix_reports,
        "task_reports": task_reports,
        "check_proofs": [],
        "required_job_ids": job_ids,
        "required_jobs": jobs,
        "assembly_errors": [],
    });
    if let Some(ledgers) = ledgers {
        request["group_ledgers"] = Value::Array(ledgers.to_vec());
    }
    let output = merge_internal_with_context(&request.to_string(), &MergeContext::detached())?;
    Ok(serde_json::from_str(&output)?)
}

pub(super) fn passed(report: &FinalReport, case: &str) -> Result<(), Box<dyn Error>> {
    if report.status != FinalStatus::Passed {
        return Err(fail(&format!(
            "{case}: status is {:?}, reasons {:?}",
            report.status, report.miss_reasons
        )));
    }
    if !report.miss_reasons.is_empty() {
        return Err(fail(&format!(
            "{case}: unexpected miss reasons {:?}",
            report.miss_reasons
        )));
    }
    Ok(())
}
