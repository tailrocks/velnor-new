// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;

use serde_json::{Value, json};
use velnor_actions_contract::{
    CacheLayer, CacheOutcome, CacheResult, ExecuteTaskIds, ExecuteTaskRef, FinalReport,
    FinalStatus, JobConclusion, JobDisposition, MatrixEntry, MatrixReport, MatrixStatus,
    MatrixTaskEntry, ObligationDecision, Plan, PlanGenerator, PlanJob, PlanMatrix, PlanObligation,
    PlanRunner, PlannedPlatform, PlatformBinding, PlatformRunnerEnvironment,
    PlatformUnavailableReason, RunnerSelection, TaskReport, TaskStatus, Trust, WorkflowEvent,
    artifact_id_for_final, canonical_json_bytes, digest_b3, plan_id_for_run, task_id_for_internal,
    task_report_id_for_task, trust_for_event,
};
use velnor_actions_orchestrator::{
    MergeContext, assemble_merge_request, merge_internal_with_context,
};

const RUN_KEY: &str = "r4242-a1";
const JOB_ID: &str = "unit-job";
const HEAD_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const OTHER_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SESSION_ID: &str = "gc1-t36-session";
const OBSERVATION_SUFFIX: &str = ".observation.json";

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
        digest_b3(b"gc1-t36-task-payload"),
        digest_b3(b"gc1-t36-input"),
        digest_b3(b"gc1-t36-closure"),
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
) -> PlanObligation {
    PlanObligation {
        task_id: task.to_owned(),
        decision: ObligationDecision::Execute,
        reason: "gc1-t36".to_owned(),
        task_digest: digest.to_owned(),
        input_digest: input_digest.to_owned(),
        closure_digest: closure_digest.to_owned(),
        job_id: JOB_ID.to_owned(),
    }
}

fn plan_with(
    task: &str,
    entry: MatrixEntry,
    obligation: PlanObligation,
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
            sha256: "e2f607dd11c96988fb44bc76b3d72c7a5d5caa687875d3826a062c0ab05cf105".to_owned(),
        },
        packages: Vec::new(),
        obligations: vec![obligation],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec![task.to_owned()],
        jobs: vec![PlanJob {
            job_id: JOB_ID.to_owned(),
            disposition: JobDisposition::Run,
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

fn check_run_fixture() -> Result<(Plan, Value, Value, MatrixEntry), Box<dyn Error>> {
    let task = task_id()?;
    let (digest, input_digest, closure_digest) = digests();
    let planned = planned_platform()?;
    let entry = run_entry(&task, &digest, &input_digest, &planned)?;
    let plan = plan_with(
        &task,
        entry.clone(),
        obligation(&task, &digest, &input_digest, &closure_digest),
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

fn member_name(matrix_key: &str, report_id: &str) -> String {
    format!("{matrix_key}/tasks/{report_id}{OBSERVATION_SUFFIX}")
}

fn observation_text(
    report_id: &str,
    artifact_id: &str,
    member: &str,
    producer_job: &str,
    attempt: u64,
    source_sha: &str,
) -> Result<String, Box<dyn Error>> {
    let content = json!({
        "schema": 1,
        "task_report_id": report_id,
        "artifact_id": artifact_id,
        "member_name": member,
        "producer_job": producer_job,
        "producer_attempt": attempt,
        "source_sha": source_sha,
        "observation_session_id": SESSION_ID,
        "unit_ledger_digest": digest_b3(b"gc1-t36-ledger"),
    });
    let content_bytes = canonical_json_bytes(&content)?;
    let binding = json!({
        "schema": 1,
        "task_report_id": report_id,
        "artifact_id": artifact_id,
        "member_name": member,
        "member_size_bytes": content_bytes.len() as u64,
        "member_digest": digest_b3(&content_bytes),
        "producer_job": producer_job,
        "producer_attempt": attempt,
        "source_sha": source_sha,
        "observation_session_id": SESSION_ID,
        "unit_ledger_digest": digest_b3(b"gc1-t36-ledger"),
    });
    Ok(serde_json::to_string(&binding)?)
}

fn valid_observation(entry: &MatrixEntry, report_id: &str) -> Result<String, Box<dyn Error>> {
    observation_text(
        report_id,
        &entry.artifact_id,
        &member_name(&entry.matrix_key, report_id),
        JOB_ID,
        1,
        HEAD_SHA,
    )
}

struct StagedTree<'a> {
    plan: &'a Plan,
    matrix: &'a Value,
    report: &'a Value,
    entry: &'a MatrixEntry,
    report_id: &'a str,
}

fn stage_tree(
    base: &Path,
    tree: &StagedTree<'_>,
    observation: Option<&str>,
    loose: bool,
) -> Result<(), Box<dyn Error>> {
    fs::write(base.join("plan.json"), serde_json::to_string(&tree.plan)?)?;
    fs::write(
        base.join("matrix.json"),
        serde_json::to_string(&tree.plan.matrix)?,
    )?;
    let home = base
        .join("reports")
        .join(&tree.entry.artifact_id)
        .join(&tree.entry.matrix_key);
    let tasks = home.join("tasks");
    fs::create_dir_all(&tasks)?;
    fs::write(
        home.join("matrix-report.json"),
        serde_json::to_string(tree.matrix)?,
    )?;
    fs::write(
        tasks.join(format!("{}.json", tree.report_id)),
        serde_json::to_string(tree.report)?,
    )?;
    if let Some(text) = observation {
        fs::write(
            tasks.join(format!("{}{OBSERVATION_SUFFIX}", tree.report_id)),
            text,
        )?;
    }
    if loose {
        fs::write(
            base.join("reports")
                .join(&tree.entry.artifact_id)
                .join("stray.observation.json"),
            "{forged loose file}",
        )?;
        fs::write(
            tasks.join("ghost.observation.json"),
            "{forged loose member}",
        )?;
    }
    Ok(())
}

fn tree_of<'a>(
    plan: &'a Plan,
    matrix: &'a Value,
    report: &'a Value,
    entry: &'a MatrixEntry,
    report_id: &'a str,
) -> StagedTree<'a> {
    StagedTree {
        plan,
        matrix,
        report,
        entry,
        report_id,
    }
}

fn assembled(base: &Path) -> Result<Value, Box<dyn Error>> {
    let request = assemble_merge_request(RUN_KEY, base)?;
    Ok(serde_json::from_str(&request)?)
}

fn assembly_errors(request: &Value) -> Vec<String> {
    request
        .get("assembly_errors")
        .and_then(Value::as_array)
        .map(|errors| {
            errors
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn observation_tokens(request: &Value) -> Vec<String> {
    assembly_errors(request)
        .into_iter()
        .filter(|error| error.contains("observation"))
        .collect()
}

#[path = "gc1_t36_a.rs"]
mod gc1_t36_a;
#[path = "gc1_t36_b.rs"]
mod gc1_t36_b;
pub(crate) use gc1_t36_a::*;
