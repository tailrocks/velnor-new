//! Task-report downstream ordering tests.

use tempfile::TempDir;
use velnor_actions_contract::{ExecuteTaskRef, Plan, TaskReport, TaskStatus};

use super::task_report_tests::{
    CLIPPY, TEST, entry_for, fixture_plan, obligation_for, read_entry, staged_run,
};
use super::*;

const JOB: &str = "crate_demo";
const DOCTEST: &str = "stack/rust/demo/doctest/default";
const DOC: &str = "stack/rust/demo/doc/default";

/// Valid fixture whose matrix order differs from its crate-job order.
fn order_fixture() -> Plan {
    let tasks = [
        (CLIPPY, "clippy", 1),
        (TEST, "test", 2),
        (DOCTEST, "doctest", 3),
        (DOC, "doc", 4),
    ];
    let mut entries = Vec::new();
    let mut obligations = Vec::new();
    for (task_id, kind, seed) in tasks {
        let (entry, task_digest) = entry_for("rust", task_id, kind, seed, JOB);
        entries.push(entry);
        obligations.push(obligation_for(task_id, task_digest, seed));
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

/// The task order that crate-job construction emits from these plan entries.
fn emitted_order(plan: &Plan) -> Vec<String> {
    let mut ordered = Vec::new();
    for entry in &plan.matrix.include {
        if entry.job_id != JOB {
            continue;
        }
        for (kind, task_ref) in &entry.execute_task_ids.tasks {
            match task_ref {
                ExecuteTaskRef::Single(id) => ordered.push(
                    crate::crate_jobs::obligation_order_key(&entry.stack_id, kind, id),
                ),
                ExecuteTaskRef::Shards(ids) => {
                    ordered.extend(ids.iter().map(|id| {
                        crate::crate_jobs::obligation_order_key(&entry.stack_id, kind, id)
                    }));
                }
            }
        }
    }
    ordered.sort_unstable();
    ordered
        .into_iter()
        .map(|(_, task_id)| task_id.to_owned())
        .collect()
}

#[test]
fn empty_downstream_follows_execution_order_not_matrix_order() {
    let plan = order_fixture();
    let matrix_order: Vec<&str> = plan
        .matrix
        .include
        .iter()
        .map(|entry| entry.task_id.as_str())
        .collect();
    assert_eq!(matrix_order, [CLIPPY, DOC, DOCTEST, TEST]);
    let emitted = emitted_order(&plan);
    assert_eq!(
        emitted,
        vec![
            CLIPPY.to_owned(),
            TEST.to_owned(),
            DOCTEST.to_owned(),
            DOC.to_owned(),
        ]
    );
    let test_position = emitted
        .iter()
        .position(|task_id| task_id == TEST)
        .expect("test in emitted order");
    assert_eq!(
        super::task_report_order::derive_downstream(&plan, TEST, JOB),
        emitted[test_position + 1..],
        "test failure must report later emitted obligations"
    );
    let doc_position = emitted
        .iter()
        .position(|task_id| task_id == DOC)
        .expect("doc in emitted order");
    assert_eq!(
        super::task_report_order::derive_downstream(&plan, DOC, JOB),
        emitted[doc_position + 1..],
        "the final emitted obligation has no downstream work"
    );

    let failed = staged_run(&plan, "local");
    assert_eq!(
        write_task_report_to("local", TEST, 1, None, &[], failed.path())
            .expect("test failure writes its actual successors"),
        3
    );
    for task_id in [DOCTEST, DOC] {
        let task = task_report(&plan, &failed, task_id);
        assert_eq!(task.status, TaskStatus::NotSelected, "{task_id}");
        assert_eq!(
            task.not_selected_reason,
            Some(velnor_actions_contract::NotSelectedReason::UpstreamFailed),
            "{task_id}"
        );
    }

    let doc_failure = staged_run(&plan, "local");
    for task_id in [TEST, DOCTEST] {
        assert_eq!(
            write_task_report_to("local", task_id, 0, None, &[], doc_failure.path())
                .expect("earlier task succeeds"),
            1
        );
    }
    assert_eq!(
        write_task_report_to("local", DOC, 1, None, &[], doc_failure.path())
            .expect("last task failure has no downstream skips"),
        1
    );
    for task_id in [TEST, DOCTEST] {
        assert_eq!(
            task_report(&plan, &doc_failure, task_id).status,
            TaskStatus::Executed,
            "doc failure must preserve previously executed {task_id}"
        );
    }
}
