//! Report-producer tests: parsing, op behavior, merge flip.
//!
//! Declared via `#[path]` from `task_report.rs` under `cfg(test)`. Plans
//! are hand-built valid values (no git fixtures); the flip test stages
//! producer output through request assembly into the merge.

use super::*;

use std::collections::BTreeMap;
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract_workflow::{ExecuteTaskRef, MatrixStatus, TaskStatus, WorkflowEvent};

#[test]
fn exit_codes_accept_the_eight_bit_range() {
    assert_eq!(parse_exit_code("0").expect("zero"), 0);
    assert_eq!(parse_exit_code("1").expect("one"), 1);
    assert_eq!(parse_exit_code("255").expect("max"), 255);
    for bad in ["", " ", "-1", "256", "99999", "0x1", "1\n", "ok"] {
        assert!(parse_exit_code(bad).is_err(), "must reject {bad:?}");
    }
}

#[test]
fn downstream_ids_split_dedupe_and_drop_blanks() {
    assert!(parse_downstream(None).is_empty());
    assert!(parse_downstream(Some("")).is_empty());
    assert_eq!(parse_downstream(Some("b,a,b,, a ,")), ["b", "a"]);
}

#[test]
fn start_stamps_parse_and_measure_or_fail_closed() {
    assert_eq!(parse_start_ms("0"), Some(0));
    assert_eq!(parse_start_ms("1759270000000"), Some(1_759_270_000_000));
    assert_eq!(parse_start_ms("99999999999999999999999999"), None);
    for bad in ["", " ", "-1", "12.5", "0x1", "abc", "1\n"] {
        assert_eq!(parse_start_ms(bad), None, "must reject {bad:?}");
    }
    assert_eq!(elapsed_ms(None), None, "absent telemetry stays unmeasured");
    assert_eq!(
        elapsed_ms(Some(u64::MAX)),
        None,
        "future stamps stay unmeasured"
    );
    assert!(
        elapsed_ms(Some(1)).is_some_and(|elapsed| elapsed >= 1),
        "past stamps measure"
    );
    let now = now_ms().expect("wall clock");
    assert!(
        elapsed_ms(Some(now)).is_some_and(|elapsed| elapsed >= 1),
        "present stamps never read zero"
    );
}

#[test]
fn executed_report_binds_plan_identities() {
    let plan = fixture_plan();
    let temp = staged_run(&plan, "local");
    let entry = &plan.matrix.include[0];
    let obligation = &plan.obligations[0];
    let reported =
        write_task_report_to("local", CLIPPY, 0, None, &[], temp.path()).expect("report");
    assert_eq!(reported, 1);
    let expect_id = velnor_actions_contract::task_report_id_for_task(
        "local",
        &entry.matrix_key,
        &obligation.task_digest,
    )
    .expect("task report id");
    let (task, matrix) = read_entry(&temp, "local", &entry.matrix_key, &expect_id);
    assert_eq!(task.status, TaskStatus::Executed);
    assert_eq!(task.exit_code, 0);
    assert_eq!(task.duration_ms, None, "missing stamp stays unmeasured");
    assert_eq!(task.task_digest, obligation.task_digest);
    assert_eq!(task.event, WorkflowEvent::PullRequest);
    assert_eq!(task.duration_ms, None, "unmeasured timing stays absent");
    assert_eq!(task.timing, None, "unmeasured timing stays absent");
    assert_eq!(matrix.status, MatrixStatus::Passed);
    assert_eq!(matrix.report_id, entry.report_id);
    assert_eq!(matrix.executed, 1);
}

#[test]
fn failed_report_marks_entry_failed() {
    let plan = fixture_plan();
    let temp = staged_run(&plan, "local");
    let entry = &plan.matrix.include[0];
    let obligation = &plan.obligations[0];
    let reported =
        write_task_report_to("local", CLIPPY, 3, Some(1), &[], temp.path()).expect("report");
    assert_eq!(reported, 1);
    let expect_id = velnor_actions_contract::task_report_id_for_task(
        "local",
        &entry.matrix_key,
        &obligation.task_digest,
    )
    .expect("task report id");
    let (task, matrix) = read_entry(&temp, "local", &entry.matrix_key, &expect_id);
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(task.exit_code, 3);
    assert!(
        task.duration_ms.is_some_and(|elapsed| elapsed >= 1),
        "present stamp measures"
    );
    let duration = task.duration_ms.expect("duration");
    let timing = task.timing.expect("measured duration carries timing");
    assert_eq!(timing.task_ms, duration);
    assert_eq!(timing.accounted_total(), duration);
    assert_eq!(
        timing.slots(),
        [0, 0, duration, 0, 0, 0, 0, 0, 0, 0],
        "only the task-body slot measures"
    );
    assert_eq!(matrix.status, MatrixStatus::Failed);
    assert_eq!(matrix.failed, 1);
}

/// Tofu task ID for the timing-carrying execution case.
const TOFU_VALIDATE: &str = "stack/tofu/stacks/a/validate/default";

#[test]
fn tofu_executed_report_carries_measured_timing() {
    let mut plan = fixture_plan();
    let (tofu_entry, digest) = entry_for("tofu", TOFU_VALIDATE, "validate", 7, "tofu_validate");
    plan.matrix.include[1] = tofu_entry;
    let entry = &plan.matrix.include[1];
    plan.obligations[1] = obligation_for(TOFU_VALIDATE, digest.clone(), 7);
    plan.task_ids[1] = TOFU_VALIDATE.to_owned();
    plan.validate().expect("mixed plan validates");
    let temp = staged_run(&plan, "local");
    let reported =
        write_task_report_to("local", TOFU_VALIDATE, 0, Some(1), &[], temp.path()).expect("report");
    assert_eq!(reported, 1);
    let expect_id =
        velnor_actions_contract::task_report_id_for_task("local", &entry.matrix_key, &digest)
            .expect("task report id");
    let (task, matrix) = read_entry(&temp, "local", &entry.matrix_key, &expect_id);
    assert_eq!(task.status, TaskStatus::Executed);
    let duration = task.duration_ms.expect("duration");
    let timing = task.timing.expect("tofu timing measures");
    assert_eq!(timing.task_ms, duration);
    assert_eq!(timing.accounted_total(), duration);
    assert_eq!(matrix.status, MatrixStatus::Passed);
}

#[test]
fn failure_reports_downstream_skips_and_success_reports_none() {
    let plan = fixture_plan();
    let failing = staged_run(&plan, "local");
    let reported =
        write_task_report_to("local", CLIPPY, 1, None, &[TEST.to_owned()], failing.path())
            .expect("report with skips");
    assert_eq!(reported, 2);
    let entry = &plan.matrix.include[1];
    let obligation = &plan.obligations[1];
    let expect_id = velnor_actions_contract::task_report_id_for_task(
        "local",
        &entry.matrix_key,
        &obligation.task_digest,
    )
    .expect("task report id");
    let (task, matrix) = read_entry(&failing, "local", &entry.matrix_key, &expect_id);
    assert_eq!(task.status, TaskStatus::NotSelected);
    assert_eq!(
        task.not_selected_reason,
        Some(velnor_actions_contract_workflow::NotSelectedReason::UpstreamFailed)
    );
    assert_eq!(matrix.not_selected, 1);

    let passing = staged_run(&plan, "local");
    let reported =
        write_task_report_to("local", CLIPPY, 0, None, &[TEST.to_owned()], passing.path())
            .expect("clean report");
    assert_eq!(reported, 1);
    assert!(
        !passing
            .path()
            .join("velnor")
            .join("local")
            .join(&entry.matrix_key)
            .exists(),
        "success reports no downstream"
    );
}

#[test]
fn unbound_inputs_fail_before_writing() {
    let plan = fixture_plan();
    let temp = staged_run(&plan, "local");
    for (run_key, task, exit) in [
        ("local", CLIPPY, 256),
        ("local", CLIPPY, -1),
        ("local", "stack/rust/demo/unknown/default", 0),
        ("r1-a1", CLIPPY, 0),
    ] {
        assert!(
            write_task_report_to(run_key, task, exit, None, &[], temp.path()).is_err(),
            "must reject {run_key}/{task}/{exit}"
        );
    }
    let missing = TempDir::new().expect("tempdir");
    assert!(write_task_report_to("local", CLIPPY, 0, None, &[], missing.path()).is_err());
    let corrupt = TempDir::new().expect("tempdir");
    let dir = corrupt.path().join("velnor").join("local");
    fs::create_dir_all(&dir).expect("run dir");
    fs::write(dir.join("plan.json"), "not json").expect("plan file");
    assert!(write_task_report_to("local", CLIPPY, 0, None, &[], corrupt.path()).is_err());
    let drifted = staged_run(&plan, "r1-a1");
    assert!(write_task_report_to("r1-a1", CLIPPY, 0, None, &[], drifted.path()).is_err());
    let rewrite = staged_run(&plan, "local");
    write_task_report_to("local", CLIPPY, 0, None, &[], rewrite.path()).expect("first write");
    assert!(write_task_report_to("local", CLIPPY, 0, None, &[], rewrite.path()).is_err());
}

#[cfg(unix)]
#[test]
fn planted_symlink_at_report_path_refuses_without_writing() {
    let plan = fixture_plan();
    let temp = staged_run(&plan, "local");
    let entry = &plan.matrix.include[0];
    let dir = temp
        .path()
        .join("velnor")
        .join("local")
        .join(&entry.matrix_key);
    let report_dir = dir.join("tasks");
    fs::create_dir_all(&report_dir).expect("report dirs");
    let loot = temp.path().join("loot.json");
    std::os::unix::fs::symlink(&loot, dir.join("matrix-report.json")).expect("plant");
    let err = write_task_report_to("local", CLIPPY, 0, None, &[], temp.path())
        .expect_err("plant refused");
    assert!(err.to_string().contains("symlink_refused"), "{err}");
    assert!(!loot.exists(), "producer bytes never followed the plant");
}

#[test]
fn ambiguous_entries_refuse_rather_than_guess() {
    let mut plan = fixture_plan();
    plan.matrix.include[0]
        .execute_task_ids
        .tasks
        .insert("extra".to_owned(), ExecuteTaskRef::Single(TEST.to_owned()));
    let multi = staged_run(&plan, "local");
    assert!(write_task_report_to("local", CLIPPY, 0, None, &[], multi.path()).is_err());

    let mut plan = fixture_plan();
    plan.matrix.include[0].execute_task_ids.tasks =
        BTreeMap::from([("only".to_owned(), ExecuteTaskRef::Single(TEST.to_owned()))]);
    let dupe = staged_run(&plan, "local");
    assert!(write_task_report_to("local", TEST, 0, None, &[], dupe.path()).is_err());

    let mut plan = fixture_plan();
    plan.obligations.clear();
    plan.task_ids.clear();
    plan.validate().expect("obligation-free plan validates");
    let orphan = staged_run(&plan, "local");
    assert!(write_task_report_to("local", CLIPPY, 0, None, &[], orphan.path()).is_err());
}
