//! Execute the generated wrapper on the actual host shell.

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use super::report_wrapper_argv;

/// Fixed private helper fixture; captured report inputs remain observable.
const HELPER: &str = r#"#!/bin/sh
case "$VELNOR_INTERNAL_OP" in
  write-task-start-v1) printf '%s' "$STAMP_VALUE"; exit "$STAMP_EXIT" ;;
  write-task-report-v1)
    printf '%s|%s\n' "$VELNOR_EXIT_CODE" "$VELNOR_START_MS" > "$REPORT_CAPTURE"
    exit "$REPORT_EXIT" ;;
  *) exit 99 ;;
esac
"#;

/// Run generator-owned argv with inherited stale telemetry and explicit outcomes.
fn run(
    task: &str,
    stamp: &str,
    stamp_exit: i32,
    report_exit: i32,
    unreadable_stamp: bool,
) -> (i32, String, Option<String>) {
    let temp = tempfile::TempDir::new().expect("temp");
    let helper = temp.path().join("helper");
    let start = temp.path().join("start");
    let capture = temp.path().join("report-inputs");
    std::fs::write(&helper, HELPER).expect("helper fixture");
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700))
        .expect("executable helper");
    if unreadable_stamp {
        std::fs::create_dir(&start).expect("stamp directory rejects writes");
    } else {
        std::fs::write(&start, "9999999999999\n").expect("old stamp");
    }
    let argv = report_wrapper_argv(
        task,
        helper.to_str().expect("helper path"),
        start.to_str().expect("stamp path"),
    );
    velnor_actions_workflow_renderer::validate_command_argv(&argv).expect("audited argv");
    let output = Command::new("/bin/sh")
        .args(&argv[1..])
        .env_clear()
        .env("STAMP_VALUE", stamp)
        .env("STAMP_EXIT", stamp_exit.to_string())
        .env("REPORT_EXIT", report_exit.to_string())
        .env("REPORT_CAPTURE", &capture)
        .env("start_ms", "1234567890123")
        .output()
        .expect("host shell");
    (
        output.status.code().expect("wrapper exit"),
        std::fs::read_to_string(capture).expect("report called"),
        std::fs::read_to_string(start).ok(),
    )
}

#[test]
fn partial_numeric_capture_is_discarded_and_task_failure_wins() {
    let (exit, inputs, stamp) = run("false", "123", 1, 9, false);
    assert_eq!(exit, 1, "task failure outranks report failure");
    assert_eq!(inputs, "1|\n", "failed capture stays unknown");
    assert_eq!(stamp.as_deref(), Some(""), "partial digits discarded");
}

#[test]
fn failed_capture_and_failed_truncate_never_reuse_inherited_stamp() {
    let (exit, inputs, stamp) = run("true", "123", 0, 0, true);
    assert_eq!(exit, 0, "unknown timing does not fail the task");
    assert_eq!(inputs, "0|\n", "untrusted stamp is never read");
    assert_eq!(stamp, None, "both write and truncate failed");
}

#[test]
fn successful_capture_hands_numeric_zero_and_report_failure_through() {
    let (exit, inputs, stamp) = run("true", "0\n", 0, 9, false);
    assert_eq!(exit, 9, "report failure surfaces after successful task");
    assert_eq!(inputs, "0|0\n", "real zero stays measured");
    assert_eq!(stamp.as_deref(), Some("0\n"));
}
