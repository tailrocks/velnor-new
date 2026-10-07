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
    let err =
        write_task_report_to("local", TASK, 0, None, &[], temp.path()).expect_err("must fail");
    assert!(
        err.to_string().contains("unparsable_plan"),
        "corrupt plan must error, never emit unbound bytes: {err}"
    );
}

#[test]
fn invalid_run_key_shapes_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    for key in ["", "UPPER", "has space", "r-a", "../escape"] {
        assert!(
            write_task_report_to(key, TASK, 0, None, &[], temp.path()).is_err(),
            "run key must validate first: {key:?}"
        );
    }
}

#[test]
fn oversize_plan_rejected_at_bound() {
    let temp = tempfile::tempdir().expect("tempdir");
    let run = temp.path().join("velnor").join("local");
    std::fs::create_dir_all(&run).expect("run dir");
    std::fs::write(run.join("plan.json"), "p".repeat(5 << 20)).expect("oversize plan");
    let err =
        write_task_report_to("local", TASK, 0, None, &[], temp.path()).expect_err("must fail");
    assert!(
        err.to_string().contains("unreadable_plan:oversize"),
        "giant plan must exhaust the bound, not memory: {err}"
    );
}

#[test]
fn plan_path_is_dir_errors() {
    let temp = tempfile::tempdir().expect("tempdir");
    let run = temp.path().join("velnor").join("local");
    std::fs::create_dir_all(run.join("plan.json")).expect("plan dir");
    let err =
        write_task_report_to("local", TASK, 0, None, &[], temp.path()).expect_err("must fail");
    assert!(
        err.to_string().contains("unreadable_plan"),
        "non-file plan errors: {err}"
    );
}

#[test]
fn empty_plan_file_errors() {
    let temp = tempfile::tempdir().expect("tempdir");
    let run = temp.path().join("velnor").join("local");
    std::fs::create_dir_all(&run).expect("run dir");
    std::fs::write(run.join("plan.json"), "").expect("empty plan");
    let err =
        write_task_report_to("local", TASK, 0, None, &[], temp.path()).expect_err("must fail");
    assert!(
        err.to_string().contains("unparsable_plan"),
        "empty plan never parses: {err}"
    );
}

#[test]
fn valid_json_wrong_shape_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let run = temp.path().join("velnor").join("local");
    std::fs::create_dir_all(&run).expect("run dir");
    std::fs::write(run.join("plan.json"), "42").expect("scalar plan");
    let err =
        write_task_report_to("local", TASK, 0, None, &[], temp.path()).expect_err("must fail");
    assert!(
        err.to_string().contains("unparsable_plan"),
        "JSON scalar is not a plan: {err}"
    );
}

#[test]
fn boundary_exits_reach_plan_stage() {
    use velnor_actions_orchestrator_core::OrchestratorError;
    let temp = tempfile::tempdir().expect("tempdir");
    for exit in [0, 255] {
        let err = write_task_report_to("local", TASK, exit, None, &[], temp.path())
            .expect_err("missing plan");
        assert!(
            matches!(err, OrchestratorError::Io { .. }),
            "boundary exit {exit} passes validation, fails only on the missing plan: {err}"
        );
    }
}
