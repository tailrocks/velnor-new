//! Verification-event request protocol regressions.

use super::{TempDir, TestResult, err_of, fs, write_request_parts};

#[test]
fn write_request_materializes_schedule_and_dispatch_scopes() -> TestResult {
    let dir = TempDir::new()?;
    let head = "f".repeat(40);
    let schedule_file = dir.path().join("schedule").join("plan-v1-request.json");
    write_request_parts(
        &schedule_file,
        "schedule",
        "{}",
        Some(&head),
        None,
        dir.path(),
    )?;
    let schedule: serde_json::Value = serde_json::from_str(&fs::read_to_string(schedule_file)?)?;
    assert_eq!(schedule["event"], "schedule");
    assert_eq!(schedule["scope"], "full");
    assert!(schedule["base"].is_null());
    assert_eq!(schedule["head"], head);

    let dispatch_file = dir.path().join("dispatch").join("plan-v1-request.json");
    let payload = r#"{"inputs":{"scope":"full"},"after":"wrong"}"#;
    write_request_parts(
        &dispatch_file,
        "workflow_dispatch",
        payload,
        Some(&head),
        None,
        dir.path(),
    )?;
    let dispatch: serde_json::Value = serde_json::from_str(&fs::read_to_string(dispatch_file)?)?;
    assert_eq!(dispatch["event"], "workflow_dispatch");
    assert_eq!(dispatch["scope"], "full");
    assert!(dispatch["base"].is_null());
    assert_eq!(dispatch["head"], head);

    let base = "a".repeat(40);
    let affected_file = dir.path().join("affected").join("plan-v1-request.json");
    let payload = format!(r#"{{"inputs":{{"scope":"affected","base_sha":"{base}"}}}}"#);
    write_request_parts(
        &affected_file,
        "workflow_dispatch",
        &payload,
        Some(&head),
        None,
        dir.path(),
    )?;
    let affected: serde_json::Value = serde_json::from_str(&fs::read_to_string(affected_file)?)?;
    assert_eq!(affected["scope"], "affected");
    assert_eq!(affected["base"], base);
    assert_eq!(affected["head"], head);
    Ok(())
}

#[test]
fn write_request_handles_optional_dispatch_base() -> TestResult {
    let dir = TempDir::new()?;
    let head = "f".repeat(40);
    for scope in ["full", "affected"] {
        let file = dir
            .path()
            .join(format!("empty-{scope}"))
            .join("plan-v1-request.json");
        let payload = format!(r#"{{"inputs":{{"scope":"{scope}","base_sha":""}}}}"#);
        write_request_parts(
            &file,
            "workflow_dispatch",
            &payload,
            Some(&head),
            None,
            dir.path(),
        )?;
        let request: serde_json::Value = serde_json::from_str(&fs::read_to_string(file)?)?;
        assert_eq!(request["scope"], scope);
        assert!(request["base"].is_null());
    }

    for (label, value) in [("null", "null"), ("integer", "7")] {
        let file = dir
            .path()
            .join(format!("bad-{label}"))
            .join("plan-v1-request.json");
        let payload = format!(r#"{{"inputs":{{"scope":"full","base_sha":{value}}}}}"#);
        let error = err_of(
            write_request_parts(
                &file,
                "workflow_dispatch",
                &payload,
                Some(&head),
                None,
                dir.path(),
            ),
            "non-string dispatch base refused",
        )?;
        assert!(error.to_string().contains("bad_base"), "{error}");
    }
    Ok(())
}

#[test]
fn write_request_rejects_bad_inputs() -> TestResult {
    let dir = TempDir::new()?;
    let payload = r#"{"before":"abc","after":"def"}"#;
    let bad_op = dir.path().join("bogus-v9-request.json");
    let err = err_of(
        write_request_parts(&bad_op, "push", payload, None, None, dir.path()),
        "unknown op refused",
    )?;
    assert!(err.to_string().contains("unknown_request_op"), "{err}");
    assert!(!bad_op.exists());
    let bad_event = dir.path().join("plan-v1-request.json");
    let err = err_of(
        write_request_parts(&bad_event, "issue_comment", payload, None, None, dir.path()),
        "unknown event refused",
    )?;
    assert!(err.to_string().contains("unsupported_event"), "{err}");
    let err = err_of(
        write_request_parts(&bad_event, "schedule", "{}", None, None, dir.path()),
        "schedule requires runner SHA",
    )?;
    assert!(err.to_string().contains("missing_schedule_head"), "{err}");
    let err = err_of(
        write_request_parts(
            &bad_event,
            "workflow_dispatch",
            r#"{"inputs":{"scope":"full","base_sha":"malformed"}}"#,
            Some(&"a".repeat(40)),
            None,
            dir.path(),
        ),
        "malformed dispatch base refused",
    )?;
    assert!(err.to_string().contains("bad_base"), "{err}");
    let err = err_of(
        write_request_parts(&bad_event, "push", "not json", None, None, dir.path()),
        "malformed payload refused",
    )?;
    assert!(err.to_string().contains("malformed_event_payload"), "{err}");

    let duplicate_scope = r#"{"inputs":{"scope":"full"},"inputs":{"scope":"affected"}}"#;
    let err = err_of(
        write_request_parts(
            &bad_event,
            "workflow_dispatch",
            duplicate_scope,
            Some(&"a".repeat(40)),
            None,
            dir.path(),
        ),
        "duplicate event fields refused",
    )?;
    assert!(err.to_string().contains("malformed_event_payload"), "{err}");

    fs::write(&bad_event, "{}")?;
    let err = err_of(
        write_request_parts(&bad_event, "push", payload, None, None, dir.path()),
        "existing file refused",
    )?;
    assert!(err.to_string().contains("request_exists"), "{err}");
    assert_eq!(fs::read_to_string(&bad_event)?, "{}");
    Ok(())
}
