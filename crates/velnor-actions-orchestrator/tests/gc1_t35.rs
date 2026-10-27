// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::io;

use serde_json::{Value, json};
use velnor_actions_contract::workflow::cache_context::{
    ExecutionBranch, JobBindingV1, UnitLedgerV1, UnitOutcome, UnitResultV1,
};
use velnor_actions_contract::{
    CacheLayer, CacheOutcome, CacheResult, Digest, ExecuteTaskIds, ExecuteTaskRef, FinalReport,
    FinalStatus, JobConclusion, JobDisposition, MatrixEntry, MatrixReport, MatrixStatus,
    MatrixTaskEntry, ObligationDecision, Plan, PlanGenerator, PlanJob, PlanMatrix, PlanObligation,
    PlanRunner, PlannedPlatform, PlatformBinding, PlatformRunnerEnvironment,
    PlatformUnavailableReason, RunnerSelection, TaskReport, TaskStatus, Trust, WorkflowEvent,
    digest_b3, plan_id_for_run, task_id_for_internal, task_report_id_for_task, trust_for_event,
};
use velnor_actions_orchestrator::{MergeContext, merge_internal_with_context};

const RUN_KEY: &str = "r4242-a1";
const JOB_ID: &str = "unit-job";
const GROUP_ID: &str = "rust-unit-job";
const GROUP_READER_ID: &str = "rust-unit-job-read";
const GROUP_WRITER_ID: &str = "rust-unit-job-write";
const SIDE_GROUP_ID: &str = "rust-side-job";
const SIDE_READER_ID: &str = "rust-side-job-read";
const SIDE_WRITER_ID: &str = "rust-side-job-write";
const HEAD_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const OTHER_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn digest(value: &[u8]) -> Result<Digest, Box<dyn Error>> {
    Ok(Digest::parse(&digest_b3(value))?)
}

fn task_id() -> Result<String, Box<dyn Error>> {
    Ok(task_id_for_internal("lint", "dev")?)
}

fn planned_platform() -> Result<PlannedPlatform, Box<dyn Error>> {
    Ok(PlannedPlatform::new(
        "ubuntu-22.04",
        "x86_64-unknown-linux-gnu",
    )?)
}

fn observed_binding(planned: &PlannedPlatform) -> Result<PlatformBinding, Box<dyn Error>> {
    Ok(PlatformBinding::unavailable(
        &planned.platform_id,
        PlatformRunnerEnvironment::GithubHosted,
        PlatformUnavailableReason::MissingImageMetadata,
    )?)
}

fn digests() -> (String, String, String) {
    (
        digest_b3(b"gc1-t35-task-payload"),
        digest_b3(b"gc1-t35-input"),
        digest_b3(b"gc1-t35-closure"),
    )
}

fn run_entry(
    task: &str,
    digest: &str,
    input_digest: &str,
    planned: &PlannedPlatform,
) -> Result<MatrixEntry, Box<dyn Error>> {
    group_run_entry(task, digest, input_digest, planned, JOB_ID)
}

fn group_run_entry(
    task: &str,
    digest: &str,
    input_digest: &str,
    planned: &PlannedPlatform,
    group: &str,
) -> Result<MatrixEntry, Box<dyn Error>> {
    let execute = ExecuteTaskIds {
        tasks: BTreeMap::from([("group".to_owned(), ExecuteTaskRef::Single(task.to_owned()))]),
    };
    Ok(MatrixEntry::derive(
        "rust",
        task,
        "ci",
        digest,
        json!({}),
        execute,
        input_digest,
        RUN_KEY,
        group,
        planned.clone(),
    )?)
}

fn obligation(
    task: &str,
    digest: &str,
    input_digest: &str,
    closure_digest: &str,
    decision: ObligationDecision,
) -> PlanObligation {
    group_obligation(task, digest, input_digest, closure_digest, decision, JOB_ID)
}

fn group_obligation(
    task: &str,
    digest: &str,
    input_digest: &str,
    closure_digest: &str,
    decision: ObligationDecision,
    group: &str,
) -> PlanObligation {
    PlanObligation {
        task_id: task.to_owned(),
        decision,
        reason: "gc1-t35".to_owned(),
        task_digest: digest.to_owned(),
        input_digest: input_digest.to_owned(),
        closure_digest: closure_digest.to_owned(),
        job_id: group.to_owned(),
    }
}

fn branch_plan(
    task: &str,
    entry: MatrixEntry,
    obligation: PlanObligation,
    group: &str,
) -> Result<Plan, Box<dyn Error>> {
    let mut plan = plan_with(task, Some(entry), obligation, JobDisposition::Run)?;
    plan.jobs = vec![PlanJob {
        job_id: group.to_owned(),
        disposition: JobDisposition::Run,
    }];
    Ok(plan)
}

fn second_task() -> Result<String, Box<dyn Error>> {
    Ok(task_id_for_internal("lint", "warm")?)
}

fn branch_typed(
    task: &str,
    group: &str,
) -> Result<(Plan, MatrixReport, TaskReport, MatrixEntry), Box<dyn Error>> {
    let (digest, input_digest, closure_digest) = digests();
    let planned = planned_platform()?;
    let entry = group_run_entry(task, &digest, &input_digest, &planned, group)?;
    let plan = branch_plan(
        task,
        entry.clone(),
        group_obligation(
            task,
            &digest,
            &input_digest,
            &closure_digest,
            ObligationDecision::Execute,
            group,
        ),
        group,
    )?;
    plan.validate()?;
    let report = task_report(task, &digest, &entry, &planned)?;
    report.validate()?;
    let matrix = matrix_report(task, &report, &entry);
    matrix.validate()?;
    Ok((plan, matrix, report, entry))
}

fn branch_evidence(task: &str, group: &str) -> Result<(Plan, Value, Value), Box<dyn Error>> {
    let (plan, matrix, report, _) = branch_typed(task, group)?;
    Ok((
        plan,
        serde_json::to_value(&matrix)?,
        serde_json::to_value(&report)?,
    ))
}

fn push_plan(plan: &Plan) -> Plan {
    let mut pushed = plan.clone();
    pushed.event = WorkflowEvent::Push;
    pushed.trust = trust_for_event(WorkflowEvent::Push);
    pushed
}

fn push_report(report: &TaskReport) -> Result<Value, Box<dyn Error>> {
    let mut pushed = report.clone();
    pushed.event = WorkflowEvent::Push;
    pushed.trust = trust_for_event(WorkflowEvent::Push);
    pushed.validate()?;
    Ok(serde_json::to_value(&pushed)?)
}

fn plan_with(
    task: &str,
    entry: Option<MatrixEntry>,
    obligation: PlanObligation,
    disposition: JobDisposition,
) -> Result<Plan, Box<dyn Error>> {
    Ok(Plan {
        schema: Plan::SCHEMA,
        run_key: RUN_KEY.to_owned(),
        plan_id: plan_id_for_run(RUN_KEY)?,
        base: None,
        head: HEAD_SHA.to_owned(),
        event: WorkflowEvent::PullRequest,
        qualification: None,
        runner: PlanRunner {
            label: "ubuntu-22.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: trust_for_event(WorkflowEvent::PullRequest),
        generator: PlanGenerator {
            version: "0.1.55".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "6dd871a3ac5473b6745b77359310384c1932ce0a7f921e06207a99d6710dcd47".to_owned(),
        },
        packages: Vec::new(),
        obligations: vec![obligation],
        matrix: PlanMatrix {
            include: entry.into_iter().collect(),
        },
        task_ids: vec![task.to_owned()],
        jobs: vec![PlanJob {
            job_id: JOB_ID.to_owned(),
            disposition,
        }],
        warnings: Vec::new(),
        edges: Vec::new(),
    })
}

fn task_report(
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

fn matrix_report(task: &str, report: &TaskReport, entry: &MatrixEntry) -> MatrixReport {
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

fn required_jobs(conclusions: &[(&str, JobConclusion)]) -> Value {
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

fn merge(
    plan: &Plan,
    matrix_reports: &[Value],
    task_reports: &[Value],
    job_ids: &[&str],
    jobs: &Value,
    run_key: &str,
    ledgers: Option<&[Value]>,
) -> Result<FinalReport, Box<dyn Error>> {
    merge_with_event(
        plan,
        matrix_reports,
        task_reports,
        job_ids,
        jobs,
        run_key,
        ledgers,
        "pull_request",
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "merge fixture mirrors the request envelope plus the observed event"
)]
fn merge_with_event(
    plan: &Plan,
    matrix_reports: &[Value],
    task_reports: &[Value],
    job_ids: &[&str],
    jobs: &Value,
    run_key: &str,
    ledgers: Option<&[Value]>,
    event: &str,
) -> Result<FinalReport, Box<dyn Error>> {
    let mut request = json!({
        "schema": 1,
        "run_key": run_key,
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

fn conclusion_of(report: &FinalReport, job_id: &str) -> Result<JobConclusion, Box<dyn Error>> {
    report
        .required_job_results
        .iter()
        .find(|job| job.job_id == job_id)
        .map(|job| job.conclusion)
        .ok_or_else(|| fail("required result missing for planned job"))
}

#[path = "gc1_t35_a.rs"]
mod gc1_t35_a;
#[path = "gc1_t35_b.rs"]
mod gc1_t35_b;
#[path = "gc1_t35_c.rs"]
mod gc1_t35_c;
#[path = "gc1_t35_d.rs"]
mod gc1_t35_d;
#[path = "gc1_t35_e.rs"]
mod gc1_t35_e;
#[path = "gc1_t35_f.rs"]
mod gc1_t35_f;
pub(crate) use gc1_t35_a::*;
pub(crate) use gc1_t35_e::*;
