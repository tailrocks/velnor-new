// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::io;

use serde_json::{Value, json};
use velnor_actions_contract::{
    CacheLayer, CacheOutcome, CacheResult, ExecuteTaskIds, ExecuteTaskRef, FinalReport,
    FinalStatus, JobConclusion, JobDisposition, MatrixEntry, MatrixReport, MatrixStatus,
    MatrixTaskEntry, ObligationDecision, Plan, PlanGenerator, PlanJob, PlanMatrix, PlanObligation,
    PlanRunner, PlannedPlatform, PlatformBinding, PlatformRunnerEnvironment,
    PlatformUnavailableReason, RunnerSelection, TaskReport, TaskStatus, Trust, WorkflowEvent,
    digest_b3, plan_id_for_run, task_id_for_internal, task_report_id_for_task, trust_for_event,
};
use velnor_actions_orchestrator::{MergeContext, merge_internal_with_context};

const RUN_KEY: &str = "r4242-a1";
const JOB_ID: &str = "unit-job";
const HEAD_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
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
        digest_b3(b"gc1-t34-task-payload"),
        digest_b3(b"gc1-t34-input"),
        digest_b3(b"gc1-t34-closure"),
    )
}

fn run_entry(
    task: &str,
    digest: &str,
    input_digest: &str,
    planned: &PlannedPlatform,
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
        JOB_ID,
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
    PlanObligation {
        task_id: task.to_owned(),
        decision,
        reason: "gc1-t34".to_owned(),
        task_digest: digest.to_owned(),
        input_digest: input_digest.to_owned(),
        closure_digest: closure_digest.to_owned(),
        job_id: JOB_ID.to_owned(),
    }
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
            sha256: "be16944e059d0cd80ac1d91b1000d694603b657387a239f75d4cfc24ca623a9f".to_owned(),
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
    jobs: impl Into<Value>,
    run_key: &str,
) -> Result<FinalReport, Box<dyn Error>> {
    let request = json!({
        "schema": 1,
        "run_key": run_key,
        "actual_event": "pull_request",
        "plan": plan,
        "matrix": plan.matrix,
        "matrix_reports": matrix_reports,
        "task_reports": task_reports,
        "check_proofs": [],
        "required_job_ids": job_ids,
        "required_jobs": jobs.into(),
        "assembly_errors": [],
    });
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

fn check_run_fixture() -> Result<(Plan, Value, Value, MatrixEntry), Box<dyn Error>> {
    let task = task_id()?;
    let (digest, input_digest, closure_digest) = digests();
    let planned = planned_platform()?;
    let entry = run_entry(&task, &digest, &input_digest, &planned)?;
    let plan = plan_with(
        &task,
        Some(entry.clone()),
        obligation(
            &task,
            &digest,
            &input_digest,
            &closure_digest,
            ObligationDecision::Execute,
        ),
        JobDisposition::Run,
    )?;
    plan.validate()?;
    let report = task_report(&task, &digest, &entry, &planned)?;
    report.validate()?;
    let matrix = matrix_report(&task, &report, &entry);
    matrix.validate()?;
    Ok((
        plan,
        serde_json::to_value(&matrix)?,
        serde_json::to_value(&report)?,
        entry,
    ))
}

fn check_run_success_preserved() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, entry) = check_run_fixture()?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        required_jobs(&[(JOB_ID, JobConclusion::Success)]),
        RUN_KEY,
    )?;
    if final_report.status != FinalStatus::Passed {
        return Err(fail(
            "successful planned unit did not preserve Required pass",
        ));
    }
    if final_report.run_key != RUN_KEY
        || final_report.plan_id != plan.plan_id
        || final_report.report_id != format!("final-{RUN_KEY}")
    {
        return Err(fail("final report lost its source plan run binding"));
    }
    if conclusion_of(&final_report, JOB_ID)? != JobConclusion::Success {
        return Err(fail("required ledger lost the successful unit conclusion"));
    }
    if !final_report.miss_reasons.is_empty() {
        return Err(fail("successful run left unexpected miss reasons"));
    }
    if final_report.downloaded_artifact_ids != [entry.artifact_id.clone()] {
        return Err(fail("successful run lost the covered artifact binding"));
    }
    if final_report.counts.selected != 1 || final_report.counts.executed != 1 {
        return Err(fail("successful run miscounted the covered unit"));
    }
    Ok(())
}

fn check_unaffected_skip_preserved() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (digest, input_digest, closure_digest) = digests();
    let plan = plan_with(
        &task,
        None,
        obligation(
            &task,
            &digest,
            &input_digest,
            &closure_digest,
            ObligationDecision::NotAffected,
        ),
        JobDisposition::NotAffected,
    )?;
    plan.validate()?;
    let final_report = merge(
        &plan,
        &[],
        &[],
        &[JOB_ID],
        required_jobs(&[(JOB_ID, JobConclusion::Skipped)]),
        RUN_KEY,
    )?;
    if final_report.status != FinalStatus::Passed {
        return Err(fail(
            "skipped unaffected unit did not preserve Required pass",
        ));
    }
    if conclusion_of(&final_report, JOB_ID)? != JobConclusion::Skipped {
        return Err(fail("required ledger lost the unaffected skip conclusion"));
    }
    Ok(())
}

fn check_untrusted_source_rejected() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, _) = check_run_fixture()?;
    let mut untrusted = plan.clone();
    untrusted.trust = Trust::Trusted;
    let final_report = merge(
        &untrusted,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        required_jobs(&[(JOB_ID, JobConclusion::Success)]),
        RUN_KEY,
    )?;
    if final_report.status != FinalStatus::PlanningFailed {
        return Err(fail("trust mismatch did not fail Required"));
    }
    if !final_report
        .miss_reasons
        .contains(&"trust_scope_mismatch".to_owned())
    {
        return Err(fail("trust mismatch lost its scope token"));
    }
    Ok(())
}

#[path = "gc1_t34_a.rs"]
mod gc1_t34_a;
#[path = "gc1_t34_b.rs"]
mod gc1_t34_b;
pub(crate) use gc1_t34_a::*;
