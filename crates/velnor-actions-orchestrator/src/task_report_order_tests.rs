//! Task-report downstream ordering tests.

use tempfile::TempDir;
use velnor_actions_contract::{Plan, TaskReport, TaskStatus};

use super::task_report_tests::{
    CLIPPY, TEST, entry_for, fixture_plan, obligation_for, read_entry, staged_run,
};
use super::*;

const JOB: &str = "crate_demo";
const DOCTEST: &str = "stack/rust/demo/doctest/default";
const DOC: &str = "stack/rust/demo/doc/default";
const BUILD: &str = "stack/rust/demo/build/default";

/// Valid fixture whose matrix order differs from its crate-job order.
fn order_fixture(with_build: bool) -> Plan {
    let mut tasks = vec![
        (CLIPPY, "clippy", 1),
        (TEST, "test", 2),
        (DOCTEST, "doctest", 3),
        (DOC, "doc", 4),
    ];
    if with_build {
        tasks.push((BUILD, "build", 5));
    }
    let mut entries = Vec::new();
    let mut obligations = Vec::new();
    for (task_id, kind, seed) in tasks {
        let (entry, task_digest) = entry_for("rust", task_id, kind, seed, JOB);
        entries.push(entry);
        obligations.push(obligation_for(task_id, task_digest, seed, JOB));
    }
    let mut plan = fixture_plan();
    plan.matrix.include = entries;
    plan.matrix
        .include
        .sort_by(|left, right| left.id.cmp(&right.id));
    plan.obligations = obligations;
    plan.obligations
        .sort_by(|left, right| left.task_id.cmp(&right.task_id));
    plan.task_ids = plan
        .obligations
        .iter()
        .map(|obligation| obligation.task_id.clone())
        .collect();
    plan.jobs = super::task_report_tests::run_jobs(&plan);
    plan.validate().expect("order fixture validates");
    plan
}

/// Read the task report associated with one task in a plan.
fn task_report(plan: &Plan, temp: &TempDir, task_id: &str) -> TaskReport {
    let entry = plan
        .matrix
        .include
        .iter()
        .find(|entry| {
            entry
                .execute_task_ids
                .tasks
                .values()
                .any(|task_ref| match task_ref {
                    velnor_actions_contract::ExecuteTaskRef::Single(id) => id == task_id,
                    velnor_actions_contract::ExecuteTaskRef::Shards(ids) => {
                        ids.iter().any(|id| id == task_id)
                    }
                })
        })
        .expect("task entry");
    let digest = plan
        .obligations
        .iter()
        .find(|obligation| obligation.task_id == task_id)
        .expect("task obligation")
        .task_digest
        .as_str();
    let report_id =
        velnor_actions_contract::task_report_id_for_task("local", &entry.matrix_key, digest)
            .expect("report ID");
    read_entry(temp, "local", &entry.matrix_key, &report_id).0
}

#[test]
fn parallel_member_failure_skips_no_sibling() {
    let plan = order_fixture(false);
    let matrix_order: Vec<&str> = plan
        .matrix
        .include
        .iter()
        .map(|entry| entry.task_id.as_str())
        .collect();
    assert_eq!(matrix_order, [CLIPPY, DOC, DOCTEST, TEST]);
    for task_id in [CLIPPY, TEST, DOCTEST, DOC] {
        assert_eq!(
            super::task_report_order::derive_downstream(&plan, task_id, JOB),
            [] as [String; 0],
            "{task_id} runs beside its siblings"
        );
    }
    let failed = staged_run(&plan, "local");
    assert_eq!(
        write_task_report_to("local", CLIPPY, 1, None, &[], failed.path())
            .expect("a failed member writes only its own report"),
        1
    );
    for task_id in [TEST, DOCTEST, DOC] {
        assert_eq!(
            write_task_report_to("local", task_id, 0, None, &[], failed.path())
                .expect("siblings still write their own reports"),
            1
        );
        assert_eq!(
            task_report(&plan, &failed, task_id).status,
            TaskStatus::Executed,
            "{task_id}"
        );
    }
}

#[test]
fn failed_build_skips_every_parallel_check() {
    let plan = order_fixture(true);
    assert_eq!(
        super::task_report_order::derive_downstream(&plan, BUILD, JOB),
        [CLIPPY, TEST, DOCTEST, DOC].map(str::to_owned),
        "the sequential build precedes the whole parallel group"
    );
    let failed = staged_run(&plan, "local");
    assert_eq!(
        write_task_report_to("local", BUILD, 1, None, &[], failed.path())
            .expect("build failure writes the skipped group"),
        5
    );
    for task_id in [CLIPPY, TEST, DOCTEST, DOC] {
        let task = task_report(&plan, &failed, task_id);
        assert_eq!(task.status, TaskStatus::NotSelected, "{task_id}");
        assert_eq!(
            task.not_selected_reason,
            Some(velnor_actions_contract::NotSelectedReason::UpstreamFailed),
            "{task_id}"
        );
    }
}
