// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! F7 READER-FIRST lifecycle repair: stage scheduling plus ledger winner semantics.
//!
//! Readers run on every event (affected-only) so a push writer never depends on
//! a skipped job under GitHub `success()` scheduling. Writers keep the trusted
//! push gate and always run when affected; the `needs_prepare` coverage signal
//! flows as ledger data, never as a job-level gate. Warm writers emit
//! cache-hit ledgers instead of preparation.

#[expect(
    dead_code,
    unused_imports,
    clippy::duplicate_mod,
    reason = "stage include covers gate helpers; only the reader-first subset is used"
)]
#[path = "../src/crate_jobs_stage.rs"]
mod crate_jobs_stage;

use std::collections::BTreeMap;
use std::error::Error;
use std::io;

use serde_json::{Value, json};
use velnor_actions_contract::workflow::cache_context::{
    ExecutionBranch, JobBindingV1, UnitLedgerV1, UnitOutcome, UnitResultV1,
};
use velnor_actions_contract::{
    CacheLayer, CacheMode, CacheOutcome, CacheResult, Digest, ExecuteTaskIds, ExecuteTaskRef,
    FinalReport, FinalStatus, Job, JobConclusion, JobDisposition, JobTimeout, MatrixEntry,
    MatrixReport, MatrixStatus, MatrixTaskEntry, ObligationDecision, Plan, PlanGenerator, PlanJob,
    PlanMatrix, PlanObligation, PlanRunner, PlannedPlatform, PlatformBinding,
    PlatformRunnerEnvironment, PlatformUnavailableReason, RunnerSelection, Step, StepKind,
    TaskReport, TaskStatus, Trust, WorkflowEvent, digest_b3, plan_id_for_run, task_id_for_internal,
    task_report_id_for_task, trust_for_event,
};
use velnor_actions_orchestrator::{MergeContext, merge_internal_with_context};

use self::crate_jobs_stage::{
    TRUSTED_WRITER_EVENT_GATE, UNTRUSTED_READER_EVENT_GATE, combined_branch_for,
    reader_branch_condition, validate_combined_branch, writer_branch_condition,
    writer_branch_needs,
};

const RUN_KEY: &str = "r4250-a1";
const GROUP_ID: &str = "rust-f7-job";
const GROUP_READER_ID: &str = "rust-f7-job-read";
const GROUP_WRITER_ID: &str = "rust-f7-job-write";
const HEAD_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn affected_gate() -> String {
    "!contains(needs.plan.outputs.not_affected_jobs, ',rust-f7-job,')".to_owned()
}

fn execution_step() -> Step {
    Step {
        name: "Run lane".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::TaskExecution {
            argv: vec!["mbx".to_owned(), "build".to_owned()],
            env: BTreeMap::new(),
            task_id: "internal/build/dev".to_owned(),
            task_digest: "digest".to_owned(),
            toolchain_inputs: velnor_actions_contract::cachekey::ToolchainInputs {
                tools: vec!["rust".to_owned()],
                components: Vec::new(),
                compile_driver: "cargo".to_owned(),
                test_runner: "cargo".to_owned(),
            },
            matrix_id: "matrix".to_owned(),
            matrix_key: "key".to_owned(),
            report_helper_version: "0.1.0".to_owned(),
            matrix_max_parallel: None,
        },
    }
}

fn base_job(
    cache_mode: CacheMode,
    condition: String,
    needs: Vec<String>,
) -> Result<Job, Box<dyn Error>> {
    Ok(Job {
        display_name: "Rust / f7".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::new(10).map_err(|_| fail("bad fixture timeout"))?,
        needs,
        condition: Some(condition),
        permissions: None,
        cache_mode: Some(cache_mode),
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        steps: vec![execution_step()],
    })
}

fn reader_first_pair() -> Result<(crate_jobs_stage::CombinedBranch, Job, Job), Box<dyn Error>> {
    let branch = combined_branch_for("rust-f7-job")?;
    let gate = affected_gate();
    let reader = base_job(
        CacheMode::explicit_read(),
        reader_branch_condition(Some(&gate))?,
        vec!["plan".to_owned()],
    )?;
    let writer = base_job(
        CacheMode::push_write_expression(),
        writer_branch_condition(Some(&gate))?,
        writer_branch_needs(&["plan".to_owned()], &branch.reader)?,
    )?;
    Ok((branch, reader, writer))
}

fn check_reader_runs_where_writer_runs() -> Result<(), Box<dyn Error>> {
    let gate = affected_gate();
    let reader_condition = reader_branch_condition(Some(&gate))?;
    let writer_condition = writer_branch_condition(Some(&gate))?;
    if reader_condition.contains(UNTRUSTED_READER_EVENT_GATE)
        || reader_condition.contains(TRUSTED_WRITER_EVENT_GATE)
    {
        return Err(fail(
            "reader carries an event gate; push writers would need a skipped job",
        ));
    }
    let expected = format!("{reader_condition} && {TRUSTED_WRITER_EVENT_GATE}");
    if writer_condition != expected {
        return Err(fail(
            "writer gate is not the reader gate plus the push trust gate; \
             a writer could run while its reader is skipped",
        ));
    }
    if reader_branch_condition(None)? != "true" {
        return Err(fail("gateless reader must run on every event"));
    }
    if writer_branch_condition(None)? != TRUSTED_WRITER_EVENT_GATE {
        return Err(fail(
            "gateless writer must keep exactly the push trust gate",
        ));
    }
    let (branch, reader, writer) = reader_first_pair()?;
    if !writer.needs.contains(&branch.reader) {
        return Err(fail("writer lost its needs edge on the reader"));
    }
    validate_combined_branch(
        &branch,
        Some(&gate),
        false,
        &crate_jobs_stage::BranchJobs {
            reader_job: &branch.reader,
            reader: &reader,
            writer_job: &branch.writer,
            writer: &writer,
        },
    )
    .map_err(|error| fail(&format!("reader-first pair rejected: {error:?}")))?;
    Ok(())
}

fn check_push_skipped_reader_rejected() -> Result<(), Box<dyn Error>> {
    let branch = combined_branch_for("rust-f7-job")?;
    let gate = affected_gate();
    let stale_reader_condition = format!("({gate}) && {UNTRUSTED_READER_EVENT_GATE}");
    let reader = base_job(
        CacheMode::explicit_read(),
        stale_reader_condition,
        vec!["plan".to_owned()],
    )?;
    let writer = base_job(
        CacheMode::push_write_expression(),
        writer_branch_condition(Some(&gate))?,
        writer_branch_needs(&["plan".to_owned()], &branch.reader)?,
    )?;
    let error = match validate_combined_branch(
        &branch,
        Some(&gate),
        false,
        &crate_jobs_stage::BranchJobs {
            reader_job: &branch.reader,
            reader: &reader,
            writer_job: &branch.writer,
            writer: &writer,
        },
    ) {
        Ok(()) => return Err(fail("push-skipped reader passed validation")),
        Err(error) => error,
    };
    if !format!("{error:?}").contains("reader_excludes_trusted_event") {
        return Err(fail(&format!(
            "push-skipped reader rejected for the wrong reason: {error:?}"
        )));
    }
    Ok(())
}

fn check_signal_flows_as_data_not_gate() -> Result<(), Box<dyn Error>> {
    let gate = affected_gate();
    for condition in [
        reader_branch_condition(Some(&gate))?,
        writer_branch_condition(Some(&gate))?,
        reader_branch_condition(None)?,
        writer_branch_condition(None)?,
    ] {
        if condition.contains("needs_prepare") || condition.contains(".result") {
            return Err(fail(
                "branch condition gates on coverage data; the needs_prepare signal \
                 must flow as ledger data inside the writer, not as a job gate",
            ));
        }
    }
    let reader_condition = reader_branch_condition(Some(&gate))?;
    if reader_condition.contains("github.event_name") {
        return Err(fail(
            "reader excludes an event; readers must run on push, pull_request, and merge_group",
        ));
    }
    let writer_condition = writer_branch_condition(Some(&gate))?;
    if !writer_condition.contains(TRUSTED_WRITER_EVENT_GATE) {
        return Err(fail("writer lost the trusted push gate"));
    }
    Ok(())
}

#[test]
fn gc1_t50_reader_first_scheduling() -> Result<(), Box<dyn Error>> {
    check_reader_runs_where_writer_runs()?;
    check_push_skipped_reader_rejected()?;
    check_signal_flows_as_data_not_gate()?;
    Ok(())
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
        digest_b3(b"gc1-t50-task-payload"),
        digest_b3(b"gc1-t50-input"),
        digest_b3(b"gc1-t50-closure"),
    )
}

fn unit_result(
    task: &str,
    outcome: UnitOutcome,
    exit_code: Option<i32>,
) -> Result<UnitResultV1, Box<dyn Error>> {
    Ok(UnitResultV1 {
        unit_id: task.to_owned(),
        recipe_id: digest(b"gc1-t50-recipe")?,
        task_identity: task.to_owned(),
        argv_digest: digest(b"gc1-t50-argv")?,
        effective_input_digest: digest(b"gc1-t50-input")?,
        outcome,
        exit_code,
        started_at: "2026-10-26T00:00:00Z".to_owned(),
        finished_at: "2026-10-26T00:00:01Z".to_owned(),
        report_digest: digest(b"gc1-t50-report")?,
    })
}

fn job_binding(job_key: &str, event: &str) -> Result<JobBindingV1, Box<dyn Error>> {
    Ok(JobBindingV1 {
        schema: 1,
        repository_id: 1,
        repository: "tailrocks/velnor-new".to_owned(),
        run_id: 4250,
        run_attempt: 1,
        job_id: 7,
        job_key: job_key.to_owned(),
        event: event.to_owned(),
        ref_name: "refs/heads/main".to_owned(),
        workflow_sha: HEAD_SHA.to_owned(),
        source_sha: HEAD_SHA.to_owned(),
        plan_digest: digest(b"gc1-t50-plan")?,
    })
}

fn group_ledger_value(
    task: &str,
    job_key: &str,
    event: &str,
    branch: ExecutionBranch,
    outcome: UnitOutcome,
    exit_code: Option<i32>,
) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::to_value(UnitLedgerV1 {
        schema: 1,
        job: job_binding(job_key, event)?,
        group_digest: digest(b"gc1-t50-group")?,
        execution_branch: branch,
        selected_unit_ids: vec![task.to_owned()],
        ordered_results: vec![unit_result(task, outcome, exit_code)?],
        observation_digest: digest(b"gc1-t50-observation")?,
        source_binding_digest: digest(b"gc1-t50-source-binding")?,
    })?)
}

fn group_entry(
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
        GROUP_ID,
        planned.clone(),
    )?)
}

fn group_obligation(
    task: &str,
    digest: &str,
    input_digest: &str,
    closure_digest: &str,
    decision: ObligationDecision,
) -> PlanObligation {
    PlanObligation {
        task_id: task.to_owned(),
        decision,
        reason: "gc1-t50".to_owned(),
        task_digest: digest.to_owned(),
        input_digest: input_digest.to_owned(),
        closure_digest: closure_digest.to_owned(),
        job_id: GROUP_ID.to_owned(),
    }
}

fn branch_plan(
    task: &str,
    entry: MatrixEntry,
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
            include: vec![entry],
        },
        task_ids: vec![task.to_owned()],
        jobs: vec![PlanJob {
            job_id: GROUP_ID.to_owned(),
            disposition,
        }],
        warnings: Vec::new(),
        edges: Vec::new(),
    })
}

fn push_plan(plan: &Plan) -> Plan {
    let mut pushed = plan.clone();
    pushed.event = WorkflowEvent::Push;
    pushed.trust = trust_for_event(WorkflowEvent::Push);
    pushed
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

fn push_report_value(report: &TaskReport) -> Result<Value, Box<dyn Error>> {
    let mut pushed = report.clone();
    pushed.event = WorkflowEvent::Push;
    pushed.trust = trust_for_event(WorkflowEvent::Push);
    pushed.validate()?;
    Ok(serde_json::to_value(&pushed)?)
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

fn branch_typed(task: &str) -> Result<(Plan, Value, TaskReport, MatrixEntry), Box<dyn Error>> {
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

fn merge_with_event(
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

fn passed(report: &FinalReport, case: &str) -> Result<(), Box<dyn Error>> {
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

fn check_push_warm_cache_hit_passes() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, typed_report, _) = branch_typed(&task)?;
    let pushed = push_plan(&plan);
    pushed.validate()?;
    let report_value = push_report_value(&typed_report)?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        "push",
        ExecutionBranch::Reader,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let writer = group_ledger_value(
        &task,
        GROUP_WRITER_ID,
        "push",
        ExecutionBranch::Writer,
        UnitOutcome::CacheHit,
        None,
    )?;
    let final_report = merge_with_event(
        &pushed,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report_value),
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        Some(&[reader, writer]),
        "push",
    )?;
    passed(
        &final_report,
        "warm writer cache-hit ledger did not preserve Required pass",
    )?;
    Ok(())
}

fn check_push_cold_writer_wins() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, typed_report, _) = branch_typed(&task)?;
    let pushed = push_plan(&plan);
    pushed.validate()?;
    let report_value = push_report_value(&typed_report)?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        "push",
        ExecutionBranch::Reader,
        UnitOutcome::NeedsPrepare,
        None,
    )?;
    let writer = group_ledger_value(
        &task,
        GROUP_WRITER_ID,
        "push",
        ExecutionBranch::Writer,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let final_report = merge_with_event(
        &pushed,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report_value),
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        Some(&[reader, writer]),
        "push",
    )?;
    passed(
        &final_report,
        "cold writer ledger did not win over the deferring reader",
    )?;
    Ok(())
}

fn check_push_absent_writer_with_reader_complete_passes() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, typed_report, _) = branch_typed(&task)?;
    let pushed = push_plan(&plan);
    pushed.validate()?;
    let report_value = push_report_value(&typed_report)?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        "push",
        ExecutionBranch::Reader,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let final_report = merge_with_event(
        &pushed,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report_value),
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        Some(std::slice::from_ref(&reader)),
        "push",
    )?;
    passed(
        &final_report,
        "reader-complete ledger with an absent writer ledger must not be cache_corrupt",
    )?;
    Ok(())
}

fn check_push_defers_without_writer_blocks() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, typed_report, _) = branch_typed(&task)?;
    let pushed = push_plan(&plan);
    pushed.validate()?;
    let report_value = push_report_value(&typed_report)?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        "push",
        ExecutionBranch::Reader,
        UnitOutcome::NeedsPrepare,
        None,
    )?;
    let final_report = merge_with_event(
        &pushed,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report_value),
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        Some(std::slice::from_ref(&reader)),
        "push",
    )?;
    if final_report.status != FinalStatus::Blocked {
        return Err(fail(
            "outstanding preparation without a writer ledger did not block",
        ));
    }
    if !final_report
        .miss_reasons
        .iter()
        .any(|reason| reason == "needs_prepare")
    {
        return Err(fail("outstanding preparation hid its miss reason"));
    }
    if final_report
        .miss_reasons
        .iter()
        .any(|reason| reason == "cache_corrupt")
    {
        return Err(fail(
            "honest-but-incomplete evidence reported as corruption",
        ));
    }
    Ok(())
}

fn check_pr_forged_writer_stays_corrupt() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix_value, typed_report, _) = branch_typed(&task)?;
    let task_value = serde_json::to_value(&typed_report)?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        "pull_request",
        ExecutionBranch::Reader,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let writer = group_ledger_value(
        &task,
        GROUP_WRITER_ID,
        "pull_request",
        ExecutionBranch::Writer,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let final_report = merge_with_event(
        &plan,
        std::slice::from_ref(&matrix_value),
        std::slice::from_ref(&task_value),
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        Some(&[reader, writer]),
        "pull_request",
    )?;
    if final_report.status != FinalStatus::PlanningFailed {
        return Err(fail(
            "off-push writer ledger next to a reader ledger did not fail",
        ));
    }
    if !final_report
        .miss_reasons
        .iter()
        .any(|reason| reason == "cache_corrupt")
    {
        return Err(fail("off-push writer ledger hid its corruption reason"));
    }
    Ok(())
}

fn check_unaffected_with_ledger_mismatches() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (digest, input_digest, closure_digest) = digests();
    let mut plan = branch_plan(
        &task,
        group_entry(&task, &digest, &input_digest, &planned_platform()?)?,
        group_obligation(
            &task,
            &digest,
            &input_digest,
            &closure_digest,
            ObligationDecision::NotAffected,
        ),
        JobDisposition::NotAffected,
    )?;
    plan.matrix.include = Vec::new();
    plan.task_ids = vec![task.clone()];
    plan.jobs = vec![PlanJob {
        job_id: GROUP_ID.to_owned(),
        disposition: JobDisposition::NotAffected,
    }];
    plan.validate()?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        "pull_request",
        ExecutionBranch::Reader,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let final_report = merge_with_event(
        &plan,
        &[],
        &[],
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Skipped),
        ]),
        Some(std::slice::from_ref(&reader)),
        "pull_request",
    )?;
    if final_report.status != FinalStatus::PlanningFailed {
        return Err(fail("ledger evidence for an unaffected group did not fail"));
    }
    if !final_report
        .miss_reasons
        .iter()
        .any(|reason| reason == "gate_mismatch")
    {
        return Err(fail("unaffected-group ledger hid its gate reason"));
    }
    Ok(())
}

#[test]
fn gc1_t50_lifecycle_ledgers() -> Result<(), Box<dyn Error>> {
    check_push_warm_cache_hit_passes()?;
    check_push_cold_writer_wins()?;
    check_push_absent_writer_with_reader_complete_passes()?;
    check_push_defers_without_writer_blocks()?;
    check_pr_forged_writer_stays_corrupt()?;
    check_unaffected_with_ledger_mismatches()?;
    Ok(())
}
