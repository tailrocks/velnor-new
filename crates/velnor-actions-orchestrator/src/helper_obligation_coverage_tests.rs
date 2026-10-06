//! Native evidence cannot rewrite upstream skips or reusable coverage.

use tempfile::TempDir;
use velnor_actions_contract::{CacheResult, TaskReport};

use super::*;

/// Read complete generated task evidence.
fn read_task(temp: &Path, plan: &Plan, entry: &MatrixEntry) -> TaskReport {
    let id = velnor_actions_contract::task_report_id_for_task(
        &plan.run_key,
        &entry.matrix_key,
        &entry.task_digest,
    )
    .expect("id");
    let path = temp
        .join("velnor")
        .join(&plan.run_key)
        .join(&entry.matrix_key)
        .join("tasks")
        .join(format!("{id}.json"));
    serde_json::from_slice(&std::fs::read(path).expect("task bytes")).expect("task")
}

#[test]
fn outcome_coverage_is_non_reusable_and_preserves_cancellation() {
    let plan = crate::task_report::task_report_tests::fixture_plan();
    let entry = &plan.matrix.include[0];
    for (outcome, status) in [
        (HelperObligationOutcome::Success, TaskStatus::Executed),
        (HelperObligationOutcome::Failure, TaskStatus::Failed),
        (HelperObligationOutcome::Cancelled, TaskStatus::Cancelled),
        (HelperObligationOutcome::Skipped, TaskStatus::NotSelected),
    ] {
        let temp = TempDir::new().expect("temp");
        write(&plan, entry, &entry.task_digest, outcome, temp.path()).expect("coverage");
        let task = read_task(temp.path(), &plan, entry);
        assert_eq!(task.status, status);
        assert_eq!(task.cache.result, CacheResult::NotAttempted);
        assert_eq!(task.duration_ms, None);
        assert!(task.outputs.is_empty());
    }
}

#[test]
fn identical_upstream_skip_preserved_but_executed_coverage_rejected() {
    let plan = crate::task_report::task_report_tests::fixture_plan();
    let entry = &plan.matrix.include[0];
    let temp = TempDir::new().expect("temp");
    skip(&plan, entry, &entry.task_digest, temp.path()).expect("upstream skip");
    write(
        &plan,
        entry,
        &entry.task_digest,
        HelperObligationOutcome::Skipped,
        temp.path(),
    )
    .expect("preserve exact skip");
    let foreign = TempDir::new().expect("temp");
    write(
        &plan,
        entry,
        &entry.task_digest,
        HelperObligationOutcome::Success,
        foreign.path(),
    )
    .expect("executed coverage");
    assert!(skip(&plan, entry, &entry.task_digest, foreign.path()).is_err());
}

#[test]
fn downstream_cannot_spoof_other_jobs_or_duplicate_current_obligation() {
    let plan = crate::task_report::task_report_tests::fixture_plan();
    let source = &plan.matrix.include[0];
    let temp = TempDir::new().expect("temp");
    assert!(
        downstream(
            &plan,
            source,
            &[plan.matrix.include[1].task_id.clone()],
            temp.path()
        )
        .is_err()
    );
    assert!(downstream(&plan, source, &[source.task_id.clone()], temp.path()).is_err());
}

#[cfg(unix)]
#[test]
fn identical_skip_beneath_symlinked_ancestor_rejected() {
    let plan = crate::task_report::task_report_tests::fixture_plan();
    let entry = &plan.matrix.include[0];
    let external = TempDir::new().expect("external");
    skip(&plan, entry, &entry.task_digest, external.path()).expect("outside skip");
    let external_task = read_task(external.path(), &plan, entry);
    let external_bytes = canonical_json_bytes(&external_task).expect("external evidence");
    for ancestor in ["matrix", "tasks"] {
        let temp = TempDir::new().expect("temp");
        let relative = Path::new("velnor")
            .join(&plan.run_key)
            .join(&entry.matrix_key);
        let (destination, source) = if ancestor == "matrix" {
            (temp.path().join(&relative), external.path().join(&relative))
        } else {
            (
                temp.path().join(&relative).join("tasks"),
                external.path().join(&relative).join("tasks"),
            )
        };
        std::fs::create_dir_all(destination.parent().expect("parent")).expect("parent dir");
        std::os::unix::fs::symlink(source, destination).expect("symlink ancestor");
        assert!(skip(&plan, entry, &entry.task_digest, temp.path()).is_err());
        assert_eq!(
            canonical_json_bytes(&read_task(external.path(), &plan, entry))
                .expect("external intact"),
            external_bytes
        );
    }
}
