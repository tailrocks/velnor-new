//! Crate-job step constructor tests.
//!
//! Declared via `#[path]` from `matrix_step.rs` under `cfg(test)`.

use super::*;

#[test]
fn report_wrapper_stamps_start_and_hands_env_to_helper() {
    let argv = report_wrapper_argv("true", "/tmp/h", "/tmp/start");
    assert_eq!(&argv[..2], ["sh".to_owned(), "-c".to_owned()]);
    let script = &argv[2];
    assert!(
        script.starts_with("date +%s%3N > \"/tmp/start\"; "),
        "start stamp first: {script}"
    );
    assert!(
        script.contains("read -r start_ms rest < \"/tmp/start\""),
        "stamp read back: {script}"
    );
    for env in [EXIT_CODE_ENV, START_MS_ENV] {
        assert!(script.contains(env), "helper env {env}: {script}");
    }
    assert!(
        script.contains(&format!("{START_MS_ENV}=\"$start_ms\"")),
        "stamp handed to helper: {script}"
    );
    assert!(
        script.ends_with("if [ \"$code\" -ne 0 ]; then exit \"$code\"; fi; exit \"$helper_code\""),
        "obligation code wins: {script}"
    );
    velnor_actions_workflow_renderer::validate_command_argv(&argv).expect("valid wrapper");
    assert!(
        !script.contains("$("),
        "no command substitution (file handoff only): {script}"
    );
}

#[test]
fn outcome_and_deferred_share_one_start_file() {
    let outcome = outcome_path_for_key("m-abc");
    let start = start_path_for_key("m-abc");
    assert_eq!(outcome, "$RUNNER_TEMP/velnor/outcome-m-abc");
    assert_eq!(start, "$RUNNER_TEMP/velnor/start-m-abc");
    let save = outcome_wrapper_argv("true", &outcome, &start);
    assert!(
        save[2].starts_with(&format!("date +%s%3N > \"{start}\"; ")),
        "outcome stamps start: {}",
        save[2]
    );
    assert!(
        save[2].contains(&format!("echo \"$code\" > \"{outcome}\"")),
        "{}",
        save[2]
    );
    let report = deferred_report_argv(&outcome, "/tmp/h", &start);
    assert!(
        report[2].contains(&format!("read -r start_ms rest < \"{start}\"")),
        "deferred reads stamp: {}",
        report[2]
    );
    assert!(
        report[2].contains(&format!("{START_MS_ENV}=\"$start_ms\"")),
        "deferred hands stamp: {}",
        report[2]
    );
    for argv in [&save, &report] {
        velnor_actions_workflow_renderer::validate_command_argv(argv).expect("valid wrapper");
        assert!(!argv[2].contains("$("), "file handoff only: {}", argv[2]);
    }
}
