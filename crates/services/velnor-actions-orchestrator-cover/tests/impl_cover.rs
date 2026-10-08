use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    digest_b3, matrix_key_for_id, report_id_for_matrix, task_report_id_for_task,
};
use velnor_actions_contract_workflow::{
    CacheLayer, CacheOutcome, CacheResult, ExecuteTaskIds, ExecuteTaskRef, MatrixEntry,
    MatrixReport, MatrixStatus, MatrixTaskEntry, TaskReport, TaskStatus, Trust, WorkflowEvent,
};
use velnor_actions_orchestrator_cover::cover::{cover_entry, partition_reports};
use velnor_actions_orchestrator_merge_ports::{CoverSinks, Fold, MergeRequest, Signals};

const TASK: &str = "stack/rust/demo/clippy/default";

/// Valid entry covering the single demo task.
fn entry_for(run_key: &str) -> MatrixEntry {
    let mut tasks = BTreeMap::new();
    tasks.insert("clippy".to_owned(), ExecuteTaskRef::Single(TASK.to_owned()));
    MatrixEntry::derive(
        "rust",
        TASK,
        "mise exec -- cargo clippy --locked",
        &digest_b3(b"task-bytes"),
        serde_json::json!({}),
        ExecuteTaskIds { tasks },
        &digest_b3(b"entry-inputs"),
        run_key,
        "plan",
    )
    .expect("entry derives")
}

/// Empty report shell bound to one entry.
fn report_shell(entry: &MatrixEntry, run_key: &str) -> MatrixReport {
    MatrixReport {
        schema: 1,
        report_id: report_id_for_matrix(run_key, &entry.matrix_key).expect("report id"),
        run_key: run_key.to_owned(),
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        status: MatrixStatus::Passed,
        expected_task_ids: Vec::new(),
        task_report_ids: Vec::new(),
        tasks: Vec::new(),
        selected: 0,
        reused: 0,
        executed: 0,
        empty_partition: 0,
        not_selected: 0,
        failed: 0,
        cancelled: 0,
    }
}

/// Merge request carrying exactly these reports.
fn request_with(reports: Vec<MatrixReport>) -> MergeRequest {
    MergeRequest {
        schema: 1,
        run_key: "local".to_owned(),
        actual_event: None,
        candidate_attestation: None,
        artifact_build_context: None,
        artifact_build_observations: Vec::new(),
        task_report_outputs: None,
        plan: None,
        matrix: None,
        matrix_reports: reports,
        task_reports: Vec::new(),
        check_proofs: Vec::new(),
        required_job_ids: Vec::new(),
        required_jobs: Vec::new(),
        assembly_errors: Vec::new(),
        baseline_manifest: None,
        shard_proofs: Vec::new(),
        limits: None,
        reference_task_ids: None,
    }
}

#[test]
fn foreign_run_key_reports_are_malformed() {
    let entry = entry_for("local");
    let mut foreign = report_shell(&entry, "r9-a1");
    foreign.report_id = report_id_for_matrix("r9-a1", &entry.matrix_key).expect("foreign id");
    let request = request_with(vec![foreign]);
    let entries = BTreeMap::from([(entry.report_id.as_str(), &entry)]);
    let mut signals = Signals::default();
    let mut miss = BTreeSet::new();
    let found = partition_reports(&request, &entries, &mut signals, &mut miss);
    assert!(found.valid.is_empty());
    assert_eq!(found.malformed, 1);
    assert!(signals.not_run);
    assert!(miss.contains("trust_scope_mismatch"));
}

#[test]
fn unknown_report_ids_fail_planning() {
    let entry = entry_for("local");
    let other_id = "stack:rust|task:stack/rust/demo/other/default";
    let other_key = matrix_key_for_id(other_id).expect("matrix key");
    let mut report = report_shell(&entry, "local");
    report.report_id = report_id_for_matrix("local", &other_key).expect("other id");
    report.matrix_id = other_id.to_owned();
    report.matrix_key = other_key;
    report.validate().expect("report validates");
    let request = request_with(vec![report]);
    let entries = BTreeMap::from([(entry.report_id.as_str(), &entry)]);
    let mut signals = Signals::default();
    let mut miss = BTreeSet::new();
    let found = partition_reports(&request, &entries, &mut signals, &mut miss);
    assert!(found.valid.is_empty());
    assert!(signals.planning_failed);
    assert!(miss.contains("cache_corrupt"));
}

#[test]
fn first_valid_report_wins_duplicates_counted() {
    let entry = entry_for("local");
    let report = report_shell(&entry, "local");
    report.validate().expect("report validates");
    let request = request_with(vec![report.clone(), report]);
    let entries = BTreeMap::from([(entry.report_id.as_str(), &entry)]);
    let mut signals = Signals::default();
    let mut miss = BTreeSet::new();
    let found = partition_reports(&request, &entries, &mut signals, &mut miss);
    assert_eq!(found.valid.len(), 1);
    assert_eq!(found.malformed, 0);
    assert_eq!(found.duplicates, 1);
}

/// Mutable sinks over caller-owned backing stores.
#[expect(
    clippy::too_many_arguments,
    reason = "one call site threads the five cover sinks"
)]
fn sinks<'a>(
    seen: &'a mut BTreeSet<String>,
    files: &'a BTreeMap<&'a str, &'a TaskReport>,
    fold: &'a mut Fold,
    signals: &'a mut Signals,
    miss: &'a mut BTreeSet<String>,
) -> CoverSinks<'a> {
    CoverSinks {
        seen_task_reports: seen,
        task_files: files,
        fold,
        signals,
        miss_reasons: miss,
    }
}

#[test]
fn binding_mismatch_rejects_entry() {
    let entry = entry_for("local");
    let mut report = report_shell(&entry, "local");
    report.matrix_id = "stack:rust|task:stack/rust/demo/other/default".to_owned();
    let request = request_with(Vec::new());
    let obligations = BTreeMap::new();
    let files = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut fold = Fold::default();
    let mut signals = Signals::default();
    let mut miss = BTreeSet::new();
    let mut owned = sinks(&mut seen, &files, &mut fold, &mut signals, &mut miss);
    assert!(!cover_entry(&request, &entry, &report, &obligations, &mut owned).expect("judges"));
    assert!(signals.planning_failed);
    assert!(signals.not_run);
    assert!(miss.contains("cache_corrupt"));
}

#[test]
fn task_set_mismatch_rejects_entry() {
    let entry = entry_for("local");
    let mut report = report_shell(&entry, "local");
    report.expected_task_ids = vec!["stack/rust/demo/other/default".to_owned()];
    let request = request_with(Vec::new());
    let obligations = BTreeMap::new();
    let files = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut fold = Fold::default();
    let mut signals = Signals::default();
    let mut miss = BTreeSet::new();
    let mut owned = sinks(&mut seen, &files, &mut fold, &mut signals, &mut miss);
    assert!(!cover_entry(&request, &entry, &report, &obligations, &mut owned).expect("judges"));
    assert!(signals.planning_failed);
    assert!(miss.contains("cache_corrupt"));
}

#[test]
fn recount_mismatch_rejects_entry() {
    let entry = entry_for("local");
    let mut report = report_shell(&entry, "local");
    report.expected_task_ids = vec![TASK.to_owned()];
    report.tasks = vec![MatrixTaskEntry {
        task_report_id: "task-local-m-0000000000000000-0000000000000000".to_owned(),
        task_id: TASK.to_owned(),
        status: TaskStatus::Executed,
        exit_code: 0,
    }];
    report.selected = 1;
    report.executed = 0;
    let request = request_with(Vec::new());
    let obligations = BTreeMap::new();
    let files = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut fold = Fold::default();
    let mut signals = Signals::default();
    let mut miss = BTreeSet::new();
    let mut owned = sinks(&mut seen, &files, &mut fold, &mut signals, &mut miss);
    assert!(!cover_entry(&request, &entry, &report, &obligations, &mut owned).expect("judges"));
    assert!(!signals.planning_failed);
    assert!(signals.not_run);
    assert!(miss.contains("cache_corrupt"));
}

/// Task file backing one executed aggregate entry.
fn task_file(report: &MatrixReport, task_id: &str, digest: &str) -> TaskReport {
    TaskReport {
        schema: 1,
        task_report_id: task_report_id_for_task(&report.run_key, &report.matrix_key, digest)
            .expect("task report id"),
        run_key: report.run_key.clone(),
        event: WorkflowEvent::PullRequest,
        trust: Trust::Pr,
        matrix_id: report.matrix_id.clone(),
        matrix_key: report.matrix_key.clone(),
        task_id: task_id.to_owned(),
        task_digest: digest.to_owned(),
        status: TaskStatus::Executed,
        not_selected_reason: None,
        cache: CacheOutcome {
            layer: CacheLayer::Task,
            key: "velnor-v1-task-pr-x".to_owned(),
            result: CacheResult::Miss,
            miss_reason: Some("no_entry".to_owned()),
        },
        exit_code: 0,
        duration_ms: Some(12),
        outputs: Vec::new(),
        lane: None,
        queue: None,
        partition: None,
        reason: None,
        timing: None,
    }
}

#[test]
fn covered_entry_folds_counts() {
    let entry = entry_for("local");
    let mut report = report_shell(&entry, "local");
    let digest = digest_b3(b"obligation-inputs");
    let task_report_id =
        task_report_id_for_task("local", &entry.matrix_key, &digest).expect("task report id");
    report.expected_task_ids = vec![TASK.to_owned()];
    report.task_report_ids = vec![task_report_id.clone()];
    report.tasks = vec![MatrixTaskEntry {
        task_report_id: task_report_id.clone(),
        task_id: TASK.to_owned(),
        status: TaskStatus::Executed,
        exit_code: 0,
    }];
    report.selected = 1;
    report.executed = 1;
    let request = request_with(Vec::new());
    let obligations = BTreeMap::from([(TASK, digest.as_str())]);
    let file = task_file(&report, TASK, &digest);
    let files = BTreeMap::from([(task_report_id.as_str(), &file)]);
    let mut seen = BTreeSet::new();
    let mut fold = Fold::default();
    let mut signals = Signals::default();
    let mut miss = BTreeSet::new();
    let mut owned = sinks(&mut seen, &files, &mut fold, &mut signals, &mut miss);
    assert!(cover_entry(&request, &entry, &report, &obligations, &mut owned).expect("judges"));
    assert_eq!(fold.executed, 1);
    assert!(miss.is_empty());
    assert!(!signals.planning_failed);
    assert!(!signals.not_run);
}
