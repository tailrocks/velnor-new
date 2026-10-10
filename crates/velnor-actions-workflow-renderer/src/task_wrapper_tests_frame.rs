use super::*;

#[cfg(unix)]
struct WrapperFrameFixture {
    root: std::path::PathBuf,
    runner_temp: std::path::PathBuf,
    frame_path: std::path::PathBuf,
    call_log: std::path::PathBuf,
    injection_marker: std::path::PathBuf,
    task_started_marker: std::path::PathBuf,
    expected_execution_digest: String,
    fields: Vec<String>,
    valid_frame: Vec<u8>,
    script: String,
}

#[cfg(unix)]
fn temporary_root() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "velnor-task-wrapper-{}-{timestamp}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ))
}

#[cfg(unix)]
fn write_executable(path: &std::path::Path, contents: &[u8]) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, contents).expect("write executable fixture");
    let mut permissions = std::fs::metadata(path)
        .expect("inspect executable fixture")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(path, permissions).expect("make fixture executable");
}

#[cfg(unix)]
fn encode_frame(fields: &[String]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for field in fields {
        bytes.extend_from_slice(field.as_bytes());
        bytes.push(0);
    }
    bytes
}

#[cfg(unix)]
fn wrapper_fields(
    task_runner: &std::path::Path,
    payload: String,
    task_started_marker: &std::path::Path,
    expected_execution_digest: &str,
) -> Vec<String> {
    vec![
        "VELNOR-TASK-EXECUTION-V1".to_owned(),
        "stack/rust/crate-0/test/default".to_owned(),
        expected_execution_digest.to_owned(),
        format!("b3-{}", "b".repeat(64)),
        "stack:rust|task:stack/rust/crate-0/test/default".to_owned(),
        "m-0123456789abcdef".to_owned(),
        VERSION.to_owned(),
        "0".to_owned(),
        String::new(),
        "2".to_owned(),
        task_runner.display().to_string(),
        payload,
        "2".to_owned(),
        "TASK_STARTED_MARKER".to_owned(),
        task_started_marker.display().to_string(),
        "TASK_EXIT_CODE".to_owned(),
        "0".to_owned(),
        "END".to_owned(),
    ]
}

#[cfg(unix)]
fn wrapper_frame_fixture() -> WrapperFrameFixture {
    let root = temporary_root();
    let runner_temp = root.join("runner temp");
    let helper_dir = runner_temp.join("velnor/bin");
    std::fs::create_dir_all(&helper_dir).expect("create fake runner temp");
    let frame_path = root.join("frame.bin");
    let call_log = root.join("helper-calls.log");
    let injection_marker = root.join("shell-injection-ran");
    let task_started_marker = root.join("task-started");
    let payload = format!("$(touch {}; printf injected)", injection_marker.display());
    let expected_execution_digest = format!("b3-{}", "a".repeat(64));
    let task_runner = root.join("task-runner");
    write_executable(
        &task_runner,
        b"#!/bin/sh\nprintf '%s' \"$1\"\n: > \"$TASK_STARTED_MARKER\"\nexit \"$TASK_EXIT_CODE\"\n",
    );
    let fields = wrapper_fields(
        &task_runner,
        payload,
        &task_started_marker,
        &expected_execution_digest,
    );
    let valid_frame = encode_frame(&fields);
    std::fs::write(&frame_path, &valid_frame).expect("write valid frame");
    let helper = helper_dir.join(format!("velnor-actions-{VERSION}"));
    write_executable(
        &helper,
        b"#!/bin/sh\nprintf '%s\\n' \"$VELNOR_INTERNAL_OP\" >> \"$VELNOR_TEST_CALL_LOG\"\ncase \"$VELNOR_INTERNAL_OP\" in\n  resolve-task-execution-v1) cat \"$VELNOR_TEST_FRAME\" ;;\n  write-task-report-v1) exit \"${VELNOR_TEST_REPORT_CODE:-0}\" ;;\n  *) exit 9 ;;\nesac\n",
    );
    let script = super::super::task_script(VERSION);
    check_wrapper_script(&root, &script);
    WrapperFrameFixture {
        root,
        runner_temp,
        frame_path,
        call_log,
        injection_marker,
        task_started_marker,
        expected_execution_digest,
        fields,
        valid_frame,
        script,
    }
}

#[cfg(unix)]
fn check_wrapper_script(root: &std::path::Path, script: &str) {
    let script_path = root.join("task-wrapper.sh");
    std::fs::write(&script_path, script).expect("write generated wrapper for syntax check");
    let syntax = std::process::Command::new("bash")
        .arg("-n")
        .arg(&script_path)
        .output()
        .expect("check generated Bash syntax");
    assert!(
        syntax.status.success(),
        "generated wrapper is not valid Bash: {}",
        String::from_utf8_lossy(&syntax.stderr)
    );
}

#[cfg(unix)]
fn run_wrapper(fixture: &WrapperFrameFixture, report_code: &str) -> std::process::Output {
    std::process::Command::new("bash")
        .arg("-x")
        .arg("-c")
        .arg(&fixture.script)
        .env("RUNNER_TEMP", &fixture.runner_temp)
        .env(super::super::RUNTIME_RUNNER_TEMP_ENV, &fixture.runner_temp)
        .env(super::super::GENERATOR_VERSION_ENV, VERSION)
        .env(
            super::super::TASK_EXECUTION_DIGEST_ENV,
            &fixture.expected_execution_digest,
        )
        .env("VELNOR_TASK_ID", "stack/rust/crate-0/test/default")
        .env("VELNOR_TEST_FRAME", &fixture.frame_path)
        .env("VELNOR_TEST_CALL_LOG", &fixture.call_log)
        .env("VELNOR_TEST_REPORT_CODE", report_code)
        .output()
        .expect("run generated Bash composite script")
}

#[cfg(unix)]
fn assert_successful_task_receives_literal_data(fixture: &WrapperFrameFixture) {
    let result = run_wrapper(fixture, "0");
    assert!(
        result.status.success(),
        "script failed: stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        result.stdout.as_slice(),
        fixture.fields[11].as_bytes(),
        "the fixed probe receives the hostile literal as one argv value; stderr={}; helper_calls={}; task_started={}",
        String::from_utf8_lossy(&result.stderr),
        std::fs::read_to_string(&fixture.call_log).unwrap_or_default(),
        fixture.task_started_marker.exists(),
    );
    assert!(!fixture.injection_marker.exists(), "payload is data");
    assert!(
        fixture.task_started_marker.exists(),
        "valid task reached probe"
    );
    std::fs::remove_file(&fixture.task_started_marker).expect("clear valid task marker");
}

#[cfg(unix)]
fn assert_task_error_precedes_report_failure(fixture: &WrapperFrameFixture) {
    let mut fields = fixture.fields.clone();
    fields[16] = "7".to_owned();
    std::fs::write(&fixture.frame_path, encode_frame(&fields)).expect("write task failure frame");
    let result = run_wrapper(fixture, "9");
    assert_eq!(result.status.code(), Some(7));
    assert!(
        fixture.task_started_marker.exists(),
        "task failure reached probe"
    );
    assert!(
        !fixture.injection_marker.exists(),
        "failure payload remained literal"
    );
    std::fs::remove_file(&fixture.task_started_marker).expect("clear task failure marker");
}

#[cfg(unix)]
fn assert_report_error_is_returned_after_success(fixture: &WrapperFrameFixture) {
    std::fs::write(&fixture.frame_path, &fixture.valid_frame).expect("restore valid frame");
    let result = run_wrapper(fixture, "9");
    assert_eq!(result.status.code(), Some(9));
    assert!(
        fixture.task_started_marker.exists(),
        "report ran after successful task"
    );
    std::fs::remove_file(&fixture.task_started_marker).expect("clear report failure marker");
}

#[cfg(unix)]
fn malformed_frames(fixture: &WrapperFrameFixture) -> Vec<(&'static str, Vec<u8>)> {
    let mut bad_count = fixture.fields.clone();
    bad_count[9] = "513".to_owned();
    let mut bad_sentinel = fixture.fields.clone();
    bad_sentinel[17] = "NOT-END".to_owned();
    let mut malformed_cap_flag = fixture.fields.clone();
    malformed_cap_flag[7] = "2".to_owned();
    let mut malformed_cap_value = fixture.fields.clone();
    malformed_cap_value[7] = "1".to_owned();
    malformed_cap_value[8] = "0".to_owned();
    let mut wrong_digest = fixture.fields.clone();
    wrong_digest[2] = format!("b3-{}", "c".repeat(64));
    let mut malformed_execution_digest = fixture.fields.clone();
    malformed_execution_digest[2] = "not-a-digest".to_owned();
    let mut malformed_task_digest = fixture.fields.clone();
    malformed_task_digest[3] = "not-a-digest".to_owned();
    let mut malformed_matrix_id = fixture.fields.clone();
    malformed_matrix_id[4] = "matrix/test".to_owned();
    let mut malformed_matrix_key = fixture.fields.clone();
    malformed_matrix_key[5] = "not-a-key".to_owned();
    let mut duplicate_env_key = fixture.fields.clone();
    duplicate_env_key[12] = "3".to_owned();
    duplicate_env_key.splice(
        17..17,
        [
            "TASK_STARTED_MARKER".to_owned(),
            fixture.task_started_marker.display().to_string(),
        ],
    );
    let mut unsupported_expression = fixture.fields.clone();
    unsupported_expression[14] = "${{ github.workspace }}/marker".to_owned();
    let mut unresolved_runner_temp = fixture.fields.clone();
    unresolved_runner_temp[11] = "${{ runner.temp }}/payload".to_owned();
    let mut unresolved_argv = unresolved_runner_temp.clone();
    unresolved_argv[10] = "${{ runner.temp }}/task-runner".to_owned();
    let mut trailing_field = fixture.valid_frame.clone();
    trailing_field.extend_from_slice(b"EXTRA\0");
    let mut missing_terminator = fixture.valid_frame.clone();
    missing_terminator.pop();
    vec![
        ("bad argv count", encode_frame(&bad_count)),
        ("bad END sentinel", encode_frame(&bad_sentinel)),
        ("malformed cap flag", encode_frame(&malformed_cap_flag)),
        ("malformed cap value", encode_frame(&malformed_cap_value)),
        ("wrong execution digest", encode_frame(&wrong_digest)),
        (
            "malformed execution digest",
            encode_frame(&malformed_execution_digest),
        ),
        (
            "malformed task digest",
            encode_frame(&malformed_task_digest),
        ),
        ("malformed matrix ID", encode_frame(&malformed_matrix_id)),
        ("malformed matrix key", encode_frame(&malformed_matrix_key)),
        ("duplicate env key", encode_frame(&duplicate_env_key)),
        (
            "unsupported expression",
            encode_frame(&unsupported_expression),
        ),
        (
            "unresolved runner.temp value",
            encode_frame(&unresolved_runner_temp),
        ),
        ("unresolved argv", encode_frame(&unresolved_argv)),
        ("trailing field", trailing_field),
        ("missing terminator", missing_terminator),
    ]
}

#[cfg(unix)]
fn assert_invalid_frames_never_start_task(fixture: &WrapperFrameFixture) {
    let invalid_frames = malformed_frames(fixture);
    for (case, frame) in &invalid_frames {
        remove_if_present(&fixture.task_started_marker);
        remove_if_present(&fixture.injection_marker);
        std::fs::write(&fixture.frame_path, frame).expect("write invalid frame");
        let result = run_wrapper(fixture, "0");
        assert_eq!(
            result.status.code(),
            Some(125),
            "{case}: malformed frame was not rejected: stdout={} stderr={}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            !fixture.task_started_marker.exists(),
            "{case}: task started"
        );
        assert!(
            !fixture.injection_marker.exists(),
            "{case}: payload evaluated"
        );
    }
    let expected_calls = expected_helper_calls(invalid_frames.len());
    assert_eq!(
        std::fs::read_to_string(&fixture.call_log).expect("record helper operations"),
        expected_calls
    );
}

#[cfg(unix)]
fn remove_if_present(path: &std::path::Path) {
    if path.exists() {
        std::fs::remove_file(path).expect("remove stale marker");
    }
}

#[cfg(unix)]
fn expected_helper_calls(invalid_frame_count: usize) -> String {
    format!(
        "resolve-task-execution-v1\nwrite-task-report-v1\nresolve-task-execution-v1\nwrite-task-report-v1\nresolve-task-execution-v1\nwrite-task-report-v1\n{}",
        "resolve-task-execution-v1\n".repeat(invalid_frame_count)
    )
}

#[cfg(unix)]
#[test]
fn wrapper_rejects_unresolved_expressions_and_passes_shell_metacharacters_as_data() {
    let fixture = wrapper_frame_fixture();
    assert_successful_task_receives_literal_data(&fixture);
    assert_task_error_precedes_report_failure(&fixture);
    assert_report_error_is_returned_after_success(&fixture);
    assert_invalid_frames_never_start_task(&fixture);
    std::fs::remove_dir_all(&fixture.root).expect("remove fake runner tree");
}
