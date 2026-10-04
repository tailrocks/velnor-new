//! Execution frame identity must bind before every evidence shortcut.
use crate::task_report::task_report_tests::{
    CLIPPY, TEST, fixture_digest, fixture_plan, staged_run,
};
use crate::task_report::write_task_report_to;
use std::{collections::BTreeMap, fs};
use tempfile::TempDir;
use velnor_actions_contract::{ExecuteTaskRef, digest_b3};

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
            write_task_report_to(
                run_key,
                task,
                &fixture_digest(task),
                exit,
                None,
                &[],
                temp.path()
            )
            .is_err(),
            "must reject {run_key}/{task}/{exit}"
        );
    }
}

#[test]
fn unreadable_plans_and_rewrites_refuse() {
    let plan = fixture_plan();
    let missing = TempDir::new().expect("tempdir");
    assert!(
        write_task_report_to(
            "local",
            CLIPPY,
            &fixture_digest(CLIPPY),
            0,
            None,
            &[],
            missing.path()
        )
        .is_err()
    );
    let corrupt = TempDir::new().expect("tempdir");
    let dir = corrupt.path().join("velnor").join("local");
    fs::create_dir_all(&dir).expect("run dir");
    fs::write(dir.join("plan.json"), "not json").expect("plan file");
    assert!(
        write_task_report_to(
            "local",
            CLIPPY,
            &fixture_digest(CLIPPY),
            0,
            None,
            &[],
            corrupt.path()
        )
        .is_err()
    );
    let drifted = staged_run(&plan, "r1-a1");
    assert!(
        write_task_report_to(
            "r1-a1",
            CLIPPY,
            &fixture_digest(CLIPPY),
            0,
            None,
            &[],
            drifted.path()
        )
        .is_err()
    );
    let rewrite = staged_run(&plan, "local");
    write_task_report_to(
        "local",
        CLIPPY,
        &fixture_digest(CLIPPY),
        0,
        None,
        &[],
        rewrite.path(),
    )
    .expect("first write");
    assert!(
        write_task_report_to(
            "local",
            CLIPPY,
            &fixture_digest(CLIPPY),
            0,
            None,
            &[],
            rewrite.path()
        )
        .is_err()
    );
}

#[test]
fn ambiguous_entries_refuse_rather_than_guess() {
    let mut plan = fixture_plan();
    plan.matrix.include[0]
        .execute_task_ids
        .tasks
        .insert("extra".to_owned(), ExecuteTaskRef::Single(TEST.to_owned()));
    let multi = staged_run(&plan, "local");
    assert!(
        write_task_report_to(
            "local",
            CLIPPY,
            &fixture_digest(CLIPPY),
            0,
            None,
            &[],
            multi.path()
        )
        .is_err()
    );

    let mut plan = fixture_plan();
    plan.matrix.include[0].execute_task_ids.tasks =
        BTreeMap::from([("only".to_owned(), ExecuteTaskRef::Single(TEST.to_owned()))]);
    let dupe = staged_run(&plan, "local");
    assert!(
        write_task_report_to(
            "local",
            TEST,
            &fixture_digest(TEST),
            0,
            None,
            &[],
            dupe.path()
        )
        .is_err()
    );

    let mut plan = fixture_plan();
    plan.obligations.clear();
    plan.task_ids.clear();
    assert!(
        plan.validate().is_err(),
        "matrix tasks require known obligations"
    );
    let orphan = staged_run(&plan, "local");
    assert!(
        write_task_report_to(
            "local",
            CLIPPY,
            &fixture_digest(CLIPPY),
            0,
            None,
            &[],
            orphan.path()
        )
        .is_err()
    );
}

#[test]
fn malformed_or_missing_execution_digest_never_writes() {
    let plan = fixture_plan();
    for digest in ["", "not-a-digest", "b3-ABC", "b3-0000"] {
        let temp = staged_run(&plan, "local");
        assert!(write_task_report_to("local", CLIPPY, digest, 0, None, &[], temp.path()).is_err());
        assert!(
            !temp
                .path()
                .join("velnor/local")
                .join(&plan.matrix.include[0].matrix_key)
                .exists()
        );
    }
}

#[test]
fn stale_execution_frame_cannot_report_new_plan_or_claim_coverage() {
    for covered in [false, true] {
        let mut plan = if covered {
            crate::task_report::task_report_cover_tests::covered_fixture()
        } else {
            fixture_plan()
        };
        let task_id = if covered { TEST } else { CLIPPY };
        let original = fixture_digest(task_id);
        let changed = digest_b3(b"new-execution-profile");
        plan.obligations
            .iter_mut()
            .find(|ob| ob.task_id == task_id)
            .expect("task")
            .task_digest = changed.clone();
        for entry in &mut plan.matrix.include {
            if entry.task_id == task_id {
                entry.task_digest = changed.clone();
            }
        }
        plan.validate().expect("new plan");
        let temp = staged_run(&plan, "local");
        let error = write_task_report_to("local", task_id, &original, 0, None, &[], temp.path())
            .expect_err("old execution cannot bind new plan");
        assert!(
            error.to_string().contains("task_digest_mismatch"),
            "{error}"
        );
        assert_eq!(
            fs::read_dir(temp.path().join("velnor/local"))
                .expect("run")
                .count(),
            2
        );
    }
}

#[test]
fn selected_entry_digest_must_equal_obligation_digest() {
    let mut plan = fixture_plan();
    plan.matrix.include[0].task_digest = digest_b3(b"another-frame");
    plan.validate()
        .expect("plan schema alone permits independent entry digest");
    let temp = staged_run(&plan, "local");
    let error = write_task_report_to(
        "local",
        CLIPPY,
        &fixture_digest(CLIPPY),
        0,
        None,
        &[],
        temp.path(),
    )
    .expect_err("entry mismatch");
    assert!(
        error.to_string().contains("task_entry_identity_mismatch"),
        "{error}"
    );
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
    let err = write_task_report_to(
        "local",
        CLIPPY,
        &fixture_digest(CLIPPY),
        0,
        None,
        &[],
        temp.path(),
    )
    .expect_err("plant refused");
    assert!(err.to_string().contains("symlink_refused"), "{err}");
    assert!(!loot.exists(), "producer bytes never followed the plant");
}

#[test]
fn digest_from_another_task_cannot_bind_requested_task() {
    let plan = fixture_plan();
    let temp = staged_run(&plan, "local");
    let error = write_task_report_to(
        "local",
        CLIPPY,
        &fixture_digest(TEST),
        0,
        None,
        &[],
        temp.path(),
    )
    .expect_err("task and digest must agree");
    assert!(
        error.to_string().contains("task_digest_mismatch"),
        "{error}"
    );
    assert!(
        !temp
            .path()
            .join("velnor/local")
            .join(&plan.matrix.include[0].matrix_key)
            .exists()
    );
}
