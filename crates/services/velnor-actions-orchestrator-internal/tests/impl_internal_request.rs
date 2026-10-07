//! Request-file materialization pins: op dispatch, canonical bytes, refusal taxonomy.
//!
//! `write_request_parts` takes every input explicitly, so the plan path
//! pins hermetically in-process. The merge path resolves its run key
//! from the environment, so that one dispatch-order case re-executes
//! this binary with a scrubbed child env (the `without_ambient_identity`
//! precedent in orchestrator-core): the parent asserts exactly one
//! passing child run, the child runs the closure.

use velnor_actions_orchestrator_internal::internal::{
    MERGE_OP, PLAN_OP, REQUEST_FILE_ENV, WRITE_REQUEST_OP, write_request_parts,
};

const PUSH_PAYLOAD: &str = r#"{"before":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","after":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}"#;

/// Marker proving the child already runs under the controlled env.
const SCRUBBED_ENV: &str = "VELNOR_INTERNAL_REQUEST_SCRUBBED";

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Run `inner` in a child with the run-key env removed.
fn with_scrubbed_child(test: &str, inner: impl FnOnce() -> TestResult) -> TestResult {
    if std::env::var(SCRUBBED_ENV).is_err() {
        let output = std::process::Command::new(std::env::current_exe()?)
            .arg(test)
            .env(SCRUBBED_ENV, "1")
            .env_remove("GITHUB_RUN_ID")
            .env_remove("GITHUB_RUN_ATTEMPT")
            .env_remove("RUNNER_TEMP")
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{test}: {stdout}{stderr}");
        assert!(stdout.contains("1 passed"), "{test}: {stdout}");
        return Ok(());
    }
    inner()
}

#[test]
fn op_tags_and_env_key_are_stable() {
    assert_eq!(PLAN_OP, "plan-v1");
    assert_eq!(MERGE_OP, "merge-v1");
    assert_eq!(WRITE_REQUEST_OP, "write-request-v1");
    assert_eq!(REQUEST_FILE_ENV, "VELNOR_REQUEST_FILE");
}

#[test]
fn unknown_op_rejected_before_payload_parse() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("bogus-request.json");
    let err = write_request_parts(&path, "push", "{not json", None, None, temp.path())
        .expect_err("must fail");
    assert!(
        err.to_string().contains("unknown_request_op"),
        "unexpected: {err}"
    );
}

#[test]
fn malformed_event_payload_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("plan-v1-request.json");
    let err = write_request_parts(&path, "push", "{not json", None, None, temp.path())
        .expect_err("must fail");
    assert!(
        err.to_string().contains("malformed_event_payload"),
        "unexpected: {err}"
    );
}

#[test]
fn unsupported_event_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("plan-v1-request.json");
    let err = write_request_parts(&path, "schedule", "{}", None, None, temp.path())
        .expect_err("must fail");
    assert!(
        err.to_string().contains("unsupported_event"),
        "unexpected: {err}"
    );
}

#[test]
fn plan_request_writes_canonical_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("plan-v1-request.json");
    let written =
        write_request_parts(&path, "push", PUSH_PAYLOAD, None, None, temp.path()).expect("write");
    assert_eq!(written, path);
    let body = std::fs::read_to_string(&path).expect("read");
    let request: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(request["schema"], 1);
    assert_eq!(request["op"], "plan-v1");
    assert_eq!(request["event"], "push");
    assert_eq!(request["root"], ".");
    assert_eq!(request["head"], "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    assert!(
        body.starts_with("{\"base\":"),
        "canonical bytes sort keys: {body}"
    );
}

#[test]
fn repository_slug_captured_or_omitted() {
    let temp = tempfile::tempdir().expect("tempdir");
    let with = temp.path().join("a").join("plan-v1-request.json");
    write_request_parts(
        &with,
        "push",
        PUSH_PAYLOAD,
        None,
        Some("octo/repo"),
        temp.path(),
    )
    .expect("write");
    let body: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&with).expect("read")).expect("json");
    assert_eq!(body["repository"], "octo/repo");
    let without = temp.path().join("b").join("plan-v1-request.json");
    write_request_parts(&without, "push", PUSH_PAYLOAD, None, None, temp.path()).expect("write");
    let body: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&without).expect("read")).expect("json");
    assert!(body.get("repository").is_none(), "omitted when unset");
    let empty = temp.path().join("c").join("plan-v1-request.json");
    write_request_parts(&empty, "push", PUSH_PAYLOAD, None, Some(""), temp.path()).expect("write");
    let body: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&empty).expect("read")).expect("json");
    assert!(body.get("repository").is_none(), "omitted when empty");
}

#[test]
fn request_file_exclusive_never_overwrites() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("plan-v1-request.json");
    write_request_parts(&path, "push", PUSH_PAYLOAD, None, None, temp.path()).expect("write");
    let err = write_request_parts(&path, "push", PUSH_PAYLOAD, None, None, temp.path())
        .expect_err("second write must fail");
    assert!(
        err.to_string().contains("request_exists"),
        "unexpected: {err}"
    );
}

#[test]
fn anchor_escape_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let anchor = temp.path().join("anchor");
    std::fs::create_dir(&anchor).expect("anchor");
    let path = temp.path().join("outside").join("plan-v1-request.json");
    assert!(
        write_request_parts(&path, "push", PUSH_PAYLOAD, None, None, &anchor).is_err(),
        "escape must fail"
    );
}

#[test]
fn merge_op_dispatches_before_event_parse() -> TestResult {
    with_scrubbed_child("merge_op_dispatches_before_event_parse", || {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("merge-v1-request.json");
        let err = write_request_parts(&path, "push", "{not json", None, None, temp.path())
            .expect_err("must fail");
        assert!(
            err.to_string().contains("missing_run_key"),
            "merge dispatch must precede event parsing: {err}"
        );
        Ok(())
    })
}
