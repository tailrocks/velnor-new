use super::*;

#[cfg(unix)]
#[test]
fn wrapper_rejects_unresolved_expressions_and_passes_shell_metacharacters_as_data() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let test_root = std::env::temp_dir().join(format!(
        "velnor-task-wrapper-{}-{timestamp}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    let runner_temp = test_root.join("runner temp");
    let helper_dir = runner_temp.join("velnor/bin");
    std::fs::create_dir_all(&helper_dir).expect("create fake runner temp");

    let helper = helper_dir.join(format!("velnor-actions-{VERSION}"));
    let task_runner = test_root.join("task-runner");
    let frame_path = test_root.join("frame.bin");
    let call_log = test_root.join("helper-calls.log");
    let injection_marker = test_root.join("shell-injection-ran");
    let task_started_marker = test_root.join("task-started");
    let payload = format!("$(touch {}; printf injected)", injection_marker.display());
    let expected_execution_digest = format!("b3-{}", "a".repeat(64));
    let plan_digest = format!("b3-{}", "b".repeat(64));
    std::fs::write(
        &task_runner,
        b"#!/bin/sh\nprintf '%s' \"$1\"\n: > \"$TASK_STARTED_MARKER\"\nexit \"$TASK_EXIT_CODE\"\n",
    )
    .expect("write fixed argv probe");
    let mut task_permissions = std::fs::metadata(&task_runner)
        .expect("inspect fixed argv probe")
        .permissions();
    task_permissions.set_mode(0o700);
    std::fs::set_permissions(&task_runner, task_permissions)
        .expect("make fixed argv probe executable");
    let fields = vec![
        "VELNOR-TASK-EXECUTION-V1".to_owned(),
        "stack/rust/crate-0/test/default".to_owned(),
        expected_execution_digest.clone(),
        plan_digest,
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
    ];
    let encode_frame = |fields: &[String]| {
        let mut bytes = Vec::new();
        for field in fields {
            bytes.extend_from_slice(field.as_bytes());
            bytes.push(0);
        }
        bytes
    };
    let valid_frame = encode_frame(&fields);
    std::fs::write(&frame_path, &valid_frame).expect("write valid frame");
    std::fs::write(
        &helper,
        b"#!/bin/sh\nprintf '%s\\n' \"$VELNOR_INTERNAL_OP\" >> \"$VELNOR_TEST_CALL_LOG\"\ncase \"$VELNOR_INTERNAL_OP\" in\n  resolve-task-execution-v1) cat \"$VELNOR_TEST_FRAME\" ;;\n  write-task-report-v1) exit \"${VELNOR_TEST_REPORT_CODE:-0}\" ;;\n  *) exit 9 ;;\nesac\n",
    )
    .expect("write fake helper");
    let mut permissions = std::fs::metadata(&helper)
        .expect("inspect fake helper")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&helper, permissions).expect("make helper executable");

    let script = super::super::task_script(VERSION);
    let script_path = test_root.join("task-wrapper.sh");
    std::fs::write(&script_path, &script).expect("write generated wrapper for syntax check");
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
    let run_script = |report_code: &str| {
        std::process::Command::new("bash")
            .arg("-x")
            .arg("-c")
            .arg(&script)
            .env("RUNNER_TEMP", &runner_temp)
            .env(super::super::RUNTIME_RUNNER_TEMP_ENV, &runner_temp)
            .env(super::super::GENERATOR_VERSION_ENV, VERSION)
            .env(
                super::super::TASK_EXECUTION_DIGEST_ENV,
                &expected_execution_digest,
            )
            .env("VELNOR_TASK_ID", "stack/rust/crate-0/test/default")
            .env("VELNOR_TEST_FRAME", &frame_path)
            .env("VELNOR_TEST_CALL_LOG", &call_log)
            .env("VELNOR_TEST_REPORT_CODE", report_code)
            .output()
            .expect("run generated Bash composite script")
    };
    let result = run_script("0");
    assert!(
        result.status.success(),
        "script failed: stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        result.stdout.as_slice(),
        fields[11].as_bytes(),
        "the fixed probe receives the hostile literal as one argv value; stderr={}; helper_calls={}; task_started={}",
        String::from_utf8_lossy(&result.stderr),
        std::fs::read_to_string(&call_log).unwrap_or_default(),
        task_started_marker.exists(),
    );
    assert!(
        !injection_marker.exists(),
        "payload is data, never shell source"
    );
    assert!(
        task_started_marker.exists(),
        "valid task reached the fixed probe"
    );
    std::fs::remove_file(&task_started_marker).expect("clear valid task marker");

    let mut task_and_report_fail = fields.clone();
    task_and_report_fail[16] = "7".to_owned();
    std::fs::write(&frame_path, encode_frame(&task_and_report_fail))
        .expect("write task failure frame");
    let task_failure = run_script("9");
    assert_eq!(task_failure.status.code(), Some(7));
    assert!(
        task_started_marker.exists(),
        "task failure reached the probe"
    );
    assert!(
        !injection_marker.exists(),
        "failure payload remained literal"
    );
    std::fs::remove_file(&task_started_marker).expect("clear task failure marker");

    std::fs::write(&frame_path, &valid_frame).expect("restore valid frame");
    let report_failure = run_script("9");
    assert_eq!(report_failure.status.code(), Some(9));
    assert!(
        task_started_marker.exists(),
        "report ran after successful task"
    );
    std::fs::remove_file(&task_started_marker).expect("clear report failure marker");

    let mut bad_count = fields.clone();
    bad_count[9] = "513".to_owned();
    let mut bad_sentinel = fields.clone();
    bad_sentinel[17] = "NOT-END".to_owned();
    let mut malformed_cap_flag = fields.clone();
    malformed_cap_flag[7] = "2".to_owned();
    let mut malformed_cap_value = fields.clone();
    malformed_cap_value[7] = "1".to_owned();
    malformed_cap_value[8] = "0".to_owned();
    let mut wrong_digest = fields.clone();
    wrong_digest[2] = format!("b3-{}", "c".repeat(64));
    let mut malformed_execution_digest = fields.clone();
    malformed_execution_digest[2] = "not-a-digest".to_owned();
    let mut malformed_task_digest = fields.clone();
    malformed_task_digest[3] = "not-a-digest".to_owned();
    let mut malformed_matrix_id = fields.clone();
    malformed_matrix_id[4] = "matrix/test".to_owned();
    let mut malformed_matrix_key = fields.clone();
    malformed_matrix_key[5] = "not-a-key".to_owned();
    let mut duplicate_env_key = fields.clone();
    duplicate_env_key[12] = "3".to_owned();
    duplicate_env_key.splice(
        17..17,
        [
            "TASK_STARTED_MARKER".to_owned(),
            task_started_marker.display().to_string(),
        ],
    );
    let mut unsupported_expression = fields.clone();
    unsupported_expression[14] = "${{ github.workspace }}/marker".to_owned();
    let mut unresolved_runner_temp = fields.clone();
    unresolved_runner_temp[11] = "${{ runner.temp }}/payload".to_owned();
    let mut unresolved_argv = unresolved_runner_temp.clone();
    unresolved_argv[10] = "${{ runner.temp }}/task-runner".to_owned();
    let mut trailing_field = valid_frame.clone();
    trailing_field.extend_from_slice(b"EXTRA\0");
    let mut missing_terminator = valid_frame;
    missing_terminator.pop();
    let invalid_frames = [
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
    ];
    let invalid_frame_count = invalid_frames.len();
    for (case, invalid_frame) in invalid_frames {
        if task_started_marker.exists() {
            std::fs::remove_file(&task_started_marker)
                .expect("clear task marker before rejection case");
        }
        if injection_marker.exists() {
            std::fs::remove_file(&injection_marker)
                .expect("clear injection marker before rejection case");
        }
        std::fs::write(&frame_path, invalid_frame).expect("write invalid frame");
        let rejected = run_script("0");
        assert_eq!(
            rejected.status.code(),
            Some(125),
            "{case}: malformed frame was not rejected: stdout={} stderr={}",
            String::from_utf8_lossy(&rejected.stdout),
            String::from_utf8_lossy(&rejected.stderr)
        );
        assert!(
            !task_started_marker.exists(),
            "{case}: invalid frame reached task execution"
        );
        assert!(
            !injection_marker.exists(),
            "{case}: invalid frame evaluated hostile argv data"
        );
    }
    let expected_calls = format!(
        "resolve-task-execution-v1\nwrite-task-report-v1\nresolve-task-execution-v1\nwrite-task-report-v1\nresolve-task-execution-v1\nwrite-task-report-v1\n{}",
        "resolve-task-execution-v1\n".repeat(invalid_frame_count)
    );
    assert_eq!(
        std::fs::read_to_string(&call_log).expect("record helper operations"),
        expected_calls
    );
    std::fs::remove_dir_all(test_root).expect("remove fake runner tree");
}
