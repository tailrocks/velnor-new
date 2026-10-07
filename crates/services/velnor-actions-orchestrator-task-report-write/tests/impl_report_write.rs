use velnor_actions_orchestrator_task_report_write::write_task_report_to;

const TASK: &str = "stack/rust/demo/clippy/default";

#[test]
fn out_of_range_exit_rejected_before_plan_read() {
    let temp = tempfile::tempdir().expect("tempdir");
    for exit in [-1, 256, 1000] {
        let err = write_task_report_to("local", TASK, exit, None, &[], temp.path())
            .expect_err("must fail");
        assert!(
            err.to_string().contains("bad_exit_code"),
            "unexpected: {err}"
        );
    }
}

#[test]
fn invalid_run_key_rejected_before_plan_read() {
    let temp = tempfile::tempdir().expect("tempdir");
    assert!(
        write_task_report_to("!!!", TASK, 0, None, &[], temp.path()).is_err(),
        "run key must validate first"
    );
}

#[test]
fn missing_plan_dir_errors() {
    let temp = tempfile::tempdir().expect("tempdir");
    assert!(
        write_task_report_to("local", TASK, 0, None, &[], temp.path()).is_err(),
        "nothing staged means no report"
    );
}

#[test]
fn corrupt_plan_errors() {
    let temp = tempfile::tempdir().expect("tempdir");
    let run = temp.path().join("velnor").join("local");
    std::fs::create_dir_all(&run).expect("run dir");
    std::fs::write(run.join("plan.json"), "{not json").expect("corrupt plan");
    assert!(
        write_task_report_to("local", TASK, 0, None, &[], temp.path()).is_err(),
        "corrupt plan must error, never emit unbound bytes"
    );
}
