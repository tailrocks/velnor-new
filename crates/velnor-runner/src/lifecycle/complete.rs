// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

use std::collections::BTreeMap;

use super::LifecycleError;
use super::acquire::JobConnection;
use super::report::ReportSummary;
use crate::exec::{Annotation, AnnotationLevel, JobResult, JobRun, StepRecord, StepResult};
use crate::job_message::AgentJobRequestMessage;
use crate::protocol::{
    RunServiceAnnotation, RunServiceAnnotationLevel, RunServiceCompleteJob, RunServiceStepResult,
    RunServiceVariableValue, TaskResult, TimelineRecordState,
};

const LOGS_DROPPED: &str =
    "Some logs were not uploaded in time and were dropped; the job result is not affected.";

fn task_result(result: JobResult) -> TaskResult {
    match result {
        JobResult::Succeeded => TaskResult::Succeeded,
        JobResult::Failed => TaskResult::Failed,
        JobResult::Cancelled => TaskResult::Canceled,
    }
}

fn step_conclusion(result: StepResult) -> TaskResult {
    match result {
        StepResult::Succeeded => TaskResult::Succeeded,
        StepResult::Failed => TaskResult::Failed,
        StepResult::Skipped => TaskResult::Skipped,
        StepResult::Cancelled => TaskResult::Canceled,
    }
}

fn annotation(annotation: &Annotation) -> RunServiceAnnotation {
    RunServiceAnnotation {
        level: match annotation.level {
            AnnotationLevel::Notice => RunServiceAnnotationLevel::Notice,
            AnnotationLevel::Warning => RunServiceAnnotationLevel::Warning,
            AnnotationLevel::Error => RunServiceAnnotationLevel::Failure,
        },
        message: annotation.message.clone(),
        title: annotation.title.clone(),
        path: annotation.file.clone(),
        start_line: annotation.line.map(i64::from),
        end_line: annotation.end_line.map(i64::from),
        start_column: annotation.column.map(i64::from),
        end_column: annotation.end_column.map(i64::from),
        step_number: annotation.step.map(i64::from),
        raw_details: None,
        is_infrastructure_issue: false,
    }
}

fn step_result(
    message: &AgentJobRequestMessage,
    run: &JobRun,
    record: &StepRecord,
    report: &ReportSummary,
) -> RunServiceStepResult {
    let times = report.times.get(&record.number);
    RunServiceStepResult {
        external_id: usize::try_from(record.number)
            .ok()
            .and_then(|number| number.checked_sub(1))
            .and_then(|index| message.steps.get(index))
            .and_then(|step| step.id.clone()),
        number: Some(i64::from(record.number)),
        name: record.name.clone(),
        action_name: None,
        reference: None,
        kind: None,
        status: TimelineRecordState::Completed,
        conclusion: step_conclusion(record.conclusion),
        started_at: times.and_then(|times| times.started_at.clone()),
        completed_at: times.and_then(|times| times.completed_at.clone()),
        completed_log_url: None,
        completed_log_lines: 0,
        annotations: run
            .annotations
            .iter()
            .filter(|annotation| annotation.step == Some(record.number))
            .map(annotation)
            .collect(),
        is_background: false,
        background_control_type: None,
        background_control_step_ids: Vec::new(),
    }
}

pub(super) fn build(
    message: &AgentJobRequestMessage,
    run: &JobRun,
    report: &ReportSummary,
) -> RunServiceCompleteJob {
    let outputs: BTreeMap<String, RunServiceVariableValue> = run
        .outputs
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                RunServiceVariableValue {
                    value: value.clone(),
                    is_secret: false,
                },
            )
        })
        .collect();
    let mut annotations: Vec<RunServiceAnnotation> = run
        .annotations
        .iter()
        .filter(|annotation| annotation.step.is_none())
        .map(annotation)
        .collect();
    if report.logs_dropped {
        annotations.push(logs_dropped_warning());
    }
    RunServiceCompleteJob {
        plan_id: message.plan.plan_id.clone(),
        job_id: message.job_id.clone(),
        conclusion: task_result(run.result),
        outputs,
        step_results: run
            .steps
            .iter()
            .map(|record| step_result(message, run, record, report))
            .collect(),
        annotations,
        telemetry: Vec::new(),
        environment_url: run.environment_url.clone(),
        billing_owner_id: message.billing_owner_id.clone(),
        infrastructure_failure_category: None,
    }
}

fn logs_dropped_warning() -> RunServiceAnnotation {
    annotation(&Annotation::new(
        AnnotationLevel::Warning,
        LOGS_DROPPED.to_owned(),
    ))
}

fn forced(
    message: &AgentJobRequestMessage,
    conclusion: TaskResult,
    level: AnnotationLevel,
    text: String,
) -> RunServiceCompleteJob {
    RunServiceCompleteJob {
        plan_id: message.plan.plan_id.clone(),
        job_id: message.job_id.clone(),
        conclusion,
        outputs: BTreeMap::new(),
        step_results: Vec::new(),
        annotations: vec![annotation(&Annotation::new(level, text))],
        telemetry: Vec::new(),
        environment_url: None,
        billing_owner_id: message.billing_owner_id.clone(),
        infrastructure_failure_category: None,
    }
}

pub(super) fn forced_failure(
    message: &AgentJobRequestMessage,
    error: &LifecycleError,
) -> RunServiceCompleteJob {
    forced(
        message,
        TaskResult::Failed,
        AnnotationLevel::Error,
        format!("The runner could not run the job: {error}"),
    )
}

pub(super) fn not_started(message: &AgentJobRequestMessage) -> RunServiceCompleteJob {
    forced(
        message,
        TaskResult::Canceled,
        AnnotationLevel::Warning,
        "The job was canceled before it started.".to_owned(),
    )
}

pub(super) fn abandoned(
    message: &AgentJobRequestMessage,
    report: &ReportSummary,
) -> RunServiceCompleteJob {
    let mut completion = forced(
        message,
        TaskResult::Canceled,
        AnnotationLevel::Warning,
        "The job did not stop within its cancellation timeout; the runner stopped it.".to_owned(),
    );
    if report.logs_dropped {
        completion.annotations.push(logs_dropped_warning());
    }
    completion
}

pub(super) async fn send(
    connection: &JobConnection,
    completion: &RunServiceCompleteJob,
) -> Result<(), LifecycleError> {
    connection
        .run_service
        .complete_job(&connection.run_service_url, completion)
        .await
        .map_err(LifecycleError::Complete)
}
