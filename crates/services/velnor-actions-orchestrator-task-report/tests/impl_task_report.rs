//! Public task-report surface: loads, resolution, derivation, writes.
use std::collections::BTreeMap;
use tempfile::TempDir;
use velnor_actions_contract::{
    artifact_id_for_crate_job, plan_id_for_run, report_id_for_matrix, task_report_id_for_task,
};
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_workflow::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, MatrixStatus, ObligationDecision, Plan,
    PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner,
    TASK_RUNTIME_RECEIPTS_DIRECTORY, TaskRuntimeIdentity, TaskRuntimeReceipt, TaskStatus, Trust,
    WorkflowEvent,
};
use velnor_actions_orchestrator_task_report::task_report::{
    derive_downstream, entry_and_digest, entry_and_digest_for_job, load_plan, terminal_task_report,
    write_entry_reports, write_entry_reports_with_runtime_receipt,
};
use velnor_actions_orchestrator_task_report::task_report_aggregate::single_task_aggregate;

const JOB: &str = "crate_demo";
const FIRST: &str = "stack/rust/demo/clippy/default";
const SECOND: &str = "stack/rust/demo/test/default";

/// Stage one `plan.json` under a fake runner temp.
fn stage(text: &str) -> TempDir {
    stage_at("local", text)
}

fn stage_at(run_key: &str, text: &str) -> TempDir {
    let dir = TempDir::new().expect("temp");
    let run = dir.path().join("velnor").join(run_key);
    std::fs::create_dir_all(&run).expect("run dir");
    std::fs::write(run.join("plan.json"), text).expect("plan");
    dir
}

/// One `b3-` digest with every byte set to `byte`.
fn digest(byte: u8) -> String {
    format!("b3-{}", format!("{byte:02x}").repeat(32))
}

fn entry_for(task_id: &str, kind: &str, seed: u8) -> (MatrixEntry, String) {
    let task_digest = digest(seed);
    let entry = MatrixEntry::derive(
        "rust",
        task_id,
        "true",
        &task_digest,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(kind.to_owned(), ExecuteTaskRef::Single(task_id.to_owned()))]),
        },
        &digest(seed + 10),
        "local",
        JOB,
    )
    .expect("entry derives");
    (entry, task_digest)
}

fn obligation_for(task_id: &str, task_digest: String, seed: u8) -> PlanObligation {
    PlanObligation {
        task_id: task_id.to_owned(),
        decision: ObligationDecision::Execute,
        reason: "selected".to_owned(),
        task_digest,
        input_digest: digest(seed + 10),
        closure_digest: digest(seed + 20),
        baseline_proof: None,
    }
}

/// Valid two-task fixture plan on one crate job.
fn fixture_plan() -> Plan {
    let (first_entry, first_digest) = entry_for(FIRST, "clippy", 1);
    let (second_entry, second_digest) = entry_for(SECOND, "test", 2);
    let plan = Plan {
        schema: 1,
        run_key: "local".to_owned(),
        plan_id: plan_id_for_run("local").expect("plan id"),
        base: None,
        head: "HEAD".to_owned(),
        event: WorkflowEvent::PullRequest,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(None).expect("baseline"),
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "a".repeat(64),
        },
        packages: Vec::new(),
        obligations: vec![
            obligation_for(FIRST, first_digest, 1),
            obligation_for(SECOND, second_digest, 2),
        ],
        matrix: PlanMatrix {
            include: vec![first_entry, second_entry],
        },
        task_ids: vec![FIRST.to_owned(), SECOND.to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),

        artifact_tasks: Vec::new(),
    };
    plan.validate().expect("fixture validates");
    plan
}

#[test]
fn missing_plan_reports_not_found() {
    let temp = TempDir::new().expect("temp");
    let err = load_plan("local", temp.path(), u64::MAX).expect_err("missing plan refuses");
    assert!(err.to_string().contains("not_found"), "{err}");
}

#[test]
fn garbage_plan_is_unparsable() {
    let err = load_plan("local", stage("{nope").path(), u64::MAX).expect_err("garbage refuses");
    assert!(err.to_string().contains("unparsable_plan"), "{err}");
}

#[test]
fn tiny_bound_rejects_before_parsing() {
    let err = load_plan("local", stage("{\"schema\":1}").path(), 1).expect_err("oversize refuses");
    assert!(err.to_string().contains("oversize"), "{err}");
}

#[test]
fn valid_plan_loads_and_binds_run_key() {
    let text = serde_json::to_string(&fixture_plan()).expect("plan json");
    let plan = load_plan("local", stage(&text).path(), u64::MAX).expect("valid plan loads");
    assert_eq!(plan.run_key, "local");
    assert_eq!(plan.task_ids.len(), 2);
}

#[test]
fn foreign_run_key_mismatches() {
    let text = serde_json::to_string(&fixture_plan()).expect("plan json");
    let err = load_plan("other", stage_at("other", &text).path(), u64::MAX)
        .expect_err("foreign run key refuses");
    assert!(err.to_string().contains("report_run_mismatch"), "{err}");
}

#[test]
fn entry_resolution_finds_task_and_digest() {
    let plan = fixture_plan();
    let (entry, digest) = entry_and_digest(&plan, FIRST).expect("entry resolves");
    assert_eq!(entry.task_id, FIRST);
    assert_eq!(
        digest,
        plan.obligations
            .iter()
            .find(|obligation| obligation.task_id == FIRST)
            .expect("obligation")
            .task_digest
    );
}

#[test]
fn unknown_task_refuses() {
    let plan = fixture_plan();
    let err = entry_and_digest(&plan, "stack/rust/demo/nope/default").expect_err("unknown refuses");
    assert!(err.to_string().contains("task_not_in_plan"), "{err}");
}

#[test]
fn job_scoped_resolution_honors_job() {
    let plan = fixture_plan();
    let (entry, _) = entry_and_digest_for_job(&plan, FIRST, JOB).expect("job entry resolves");
    assert_eq!(entry.job_id, JOB);
    let err =
        entry_and_digest_for_job(&plan, FIRST, "crate_other").expect_err("foreign job refuses");
    assert!(
        err.to_string().contains("task_not_in_plan_for_job"),
        "{err}"
    );
}

#[test]
fn terminal_report_marks_executed_or_failed() {
    let plan = fixture_plan();
    let (entry, digest) = entry_and_digest(&plan, FIRST).expect("entry resolves");
    let executed =
        terminal_task_report(&plan, entry, digest, 0, Some(1)).expect("executed derives");
    assert_eq!(executed.status, TaskStatus::Executed);
    let failed = terminal_task_report(&plan, entry, digest, 3, Some(1)).expect("failed derives");
    assert_eq!(failed.status, TaskStatus::Failed);
    assert_eq!(failed.exit_code, 3);
}

#[test]
fn aggregate_counts_single_task() {
    let plan = fixture_plan();
    let (entry, digest) = entry_and_digest(&plan, FIRST).expect("entry resolves");
    let task = terminal_task_report(&plan, entry, digest, 0, Some(1)).expect("task derives");
    let matrix = single_task_aggregate(&plan, entry, &task).expect("aggregate derives");
    assert_eq!(matrix.status, MatrixStatus::Passed);
    assert_eq!(matrix.task_report_ids, vec![task.task_report_id.clone()]);
    assert_eq!(matrix.executed, 1);
}

#[test]
fn entry_reports_write_canonical_files() {
    let plan = fixture_plan();
    let (entry, digest) = entry_and_digest(&plan, FIRST).expect("entry resolves");
    let task = terminal_task_report(&plan, entry, digest, 0, Some(1)).expect("task derives");
    let matrix = single_task_aggregate(&plan, entry, &task).expect("aggregate derives");
    let temp = TempDir::new().expect("temp");
    write_entry_reports(temp.path(), &plan, entry, &task, &matrix).expect("reports write");
    let dir = temp
        .path()
        .join("velnor")
        .join(&plan.run_key)
        .join(&entry.matrix_key);
    assert!(dir.join("matrix-report.json").is_file());
    assert!(
        dir.join("tasks")
            .join(format!("{}.json", task.task_report_id))
            .is_file()
    );
}

#[test]
fn downstream_follows_execution_order() {
    let plan = fixture_plan();
    assert_eq!(
        derive_downstream(&plan, FIRST, JOB),
        vec![SECOND.to_owned()]
    );
    assert!(derive_downstream(&plan, SECOND, JOB).is_empty());
}

#[test]
fn runtime_receipt_sidecar_binds_plan_entry_without_changing_task_report() {
    let mut plan = fixture_plan();
    plan.run_key = "r123-a2".to_owned();
    plan.plan_id = plan_id_for_run(&plan.run_key).expect("plan id");
    plan.head = "a".repeat(40);
    for entry in &mut plan.matrix.include {
        entry.report_id =
            report_id_for_matrix(&plan.run_key, &entry.matrix_key).expect("report id");
        entry.artifact_id =
            artifact_id_for_crate_job(&plan.run_key, &entry.job_id).expect("artifact name");
    }
    plan.validate().expect("remote plan validates");
    let (entry, digest) = entry_and_digest(&plan, FIRST).expect("entry resolves");
    let task = terminal_task_report(&plan, entry, digest, 0, Some(1)).expect("task derives");
    let matrix = single_task_aggregate(&plan, entry, &task).expect("aggregate derives");
    let runtime = TaskRuntimeIdentity::new(
        "org/repo".to_owned(),
        "123".to_owned(),
        2,
        plan.head.clone(),
        "org/repo/.github/workflows/ci.yml@refs/pull/4/merge".to_owned(),
        JOB.to_owned(),
        "runner-17".to_owned(),
    )
    .expect("runtime identity");
    let receipt = TaskRuntimeReceipt::derive(&plan, entry, &task.task_report_id, &runtime)
        .expect("bound receipt");
    let temp = TempDir::new().expect("temp");
    write_entry_reports_with_runtime_receipt(
        temp.path(),
        &plan,
        entry,
        &task,
        &matrix,
        Some(&receipt),
    )
    .expect("reports and sidecar write");
    let dir = temp
        .path()
        .join("velnor")
        .join(&plan.run_key)
        .join(&entry.matrix_key);
    let task_bytes = std::fs::read(
        dir.join("tasks")
            .join(format!("{}.json", task.task_report_id)),
    )
    .expect("task report bytes");
    let task_json: serde_json::Value = serde_json::from_slice(&task_bytes).expect("task json");
    assert!(task_json.get("runtime_receipt").is_none());
    let receipt_path = dir
        .join(TASK_RUNTIME_RECEIPTS_DIRECTORY)
        .join(format!("{}.json", task.task_report_id));
    let receipt_bytes = std::fs::read(receipt_path).expect("receipt sidecar");
    let decoded: TaskRuntimeReceipt = serde_json::from_slice(&receipt_bytes).expect("receipt json");
    decoded
        .validate_for_plan_entry(&plan, entry)
        .expect("receipt remains plan-bound");
    assert_eq!(decoded.workflow_job_key, entry.job_id);
    assert_eq!(decoded.report_artifact_name, entry.artifact_id);
    assert_eq!(decoded.runner_name, "runner-17");
}

#[test]
fn runtime_receipt_rejects_foreign_entry_and_orphan_obligation() {
    let mut plan = fixture_plan();
    plan.run_key = "r123-a2".to_owned();
    plan.plan_id = plan_id_for_run(&plan.run_key).expect("plan id");
    plan.head = "a".repeat(40);
    for entry in &mut plan.matrix.include {
        entry.report_id =
            report_id_for_matrix(&plan.run_key, &entry.matrix_key).expect("report id");
        entry.artifact_id =
            artifact_id_for_crate_job(&plan.run_key, &entry.job_id).expect("artifact name");
    }
    plan.validate().expect("remote plan validates");
    let (entry, entry_digest) = entry_and_digest(&plan, FIRST).expect("entry resolves");
    let report_id = task_report_id_for_task(&plan.run_key, &entry.matrix_key, entry_digest)
        .expect("task report id");
    let runtime = TaskRuntimeIdentity::new(
        "org/repo".to_owned(),
        "123".to_owned(),
        2,
        plan.head.clone(),
        "org/repo/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        JOB.to_owned(),
        "runner-17".to_owned(),
    )
    .expect("runtime identity");

    let mut foreign = entry.clone();
    foreign.job_id = "other_job".to_owned();
    foreign.artifact_id =
        artifact_id_for_crate_job(&plan.run_key, &foreign.job_id).expect("foreign artifact");
    let foreign_runtime = TaskRuntimeIdentity::new(
        "org/repo".to_owned(),
        "123".to_owned(),
        2,
        plan.head.clone(),
        "org/repo/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        foreign.job_id.clone(),
        "runner-17".to_owned(),
    )
    .expect("foreign runtime identity");
    assert!(TaskRuntimeReceipt::derive(&plan, &foreign, &report_id, &foreign_runtime).is_err());

    let mut forged = TaskRuntimeReceipt::derive(&plan, entry, &report_id, &runtime)
        .expect("authoritative receipt");
    forged.plan_job_id = foreign.job_id.clone();
    forged.report_artifact_name = foreign.artifact_id.clone();
    forged.workflow_job_key = foreign.job_id.clone();
    assert!(forged.validate_for_plan_entry(&plan, &foreign).is_err());

    let mut inconsistent = plan.clone();
    inconsistent
        .obligations
        .iter_mut()
        .find(|obligation| obligation.task_id == FIRST)
        .expect("matching obligation")
        .task_digest = digest(99);
    inconsistent
        .validate()
        .expect("structural plan remains valid");
    assert!(TaskRuntimeReceipt::derive(&inconsistent, entry, &report_id, &runtime).is_err());
}

#[test]
fn runtime_receipt_rejects_source_or_workflow_job_mismatch() {
    let mut plan = fixture_plan();
    plan.run_key = "r123-a2".to_owned();
    plan.plan_id = plan_id_for_run(&plan.run_key).expect("plan id");
    plan.head = "a".repeat(40);
    for entry in &mut plan.matrix.include {
        entry.report_id =
            report_id_for_matrix(&plan.run_key, &entry.matrix_key).expect("report id");
        entry.artifact_id =
            artifact_id_for_crate_job(&plan.run_key, &entry.job_id).expect("artifact name");
    }
    plan.validate().expect("remote plan validates");
    let (entry, digest) = entry_and_digest(&plan, FIRST).expect("entry resolves");
    let task = terminal_task_report(&plan, entry, digest, 0, None).expect("task derives");
    let wrong_job = TaskRuntimeIdentity::new(
        "org/repo".to_owned(),
        "123".to_owned(),
        2,
        plan.head.clone(),
        "org/repo/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        "other_job".to_owned(),
        "runner-17".to_owned(),
    )
    .expect("syntactically valid runtime identity");
    assert!(TaskRuntimeReceipt::derive(&plan, entry, &task.task_report_id, &wrong_job).is_err());
    let wrong_source = TaskRuntimeIdentity::new(
        "org/repo".to_owned(),
        "123".to_owned(),
        2,
        "b".repeat(40),
        "org/repo/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        JOB.to_owned(),
        "runner-17".to_owned(),
    )
    .expect("syntactically valid runtime identity");
    assert!(TaskRuntimeReceipt::derive(&plan, entry, &task.task_report_id, &wrong_source).is_err());
}
