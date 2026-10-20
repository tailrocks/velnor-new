//! Poll envelope statistics tests.

use velnor_runner_github::{
    CAPACITY_HEADER, Certainty, InnerJob, MAX_POLL_BODY_BYTES, Method, Poll, RefreshGate,
    TransportFail, WireError, parse_poll, poll,
};

mod common;

use common::{QUEUE, Script, header};

// External Apache-2.0 fixture: velnor-reference 1b9fc038, SHA-256 edc857a568346af88e2447511c752a55c1730e3971f39f36699c3258cfa76c48.
const STATISTICS_ONLY: &str = include_str!("fixtures/scaleset/message_stats_only.json");
const STATISTICS: &str = r#""statistics":{"totalAcquiredJobs":0,"totalAssignedJobs":2,"totalAvailableJobs":1,"totalBusyRunners":1,"totalIdleRunners":1,"totalRegisteredRunners":2,"totalRunningJobs":1}"#;

#[test]
fn statistics_only_fixture_uses_assigned_population() {
    let Poll::Batch(batch) = parse_poll(200, STATISTICS_ONLY).expect("valid statistics-only poll")
    else {
        panic!("statistics-only poll must produce a batch");
    };
    assert_eq!(batch.message_id, 2);
    assert_eq!(batch.jobs, Vec::<InnerJob>::new());
    assert_eq!(batch.raw_body, "");
    assert_eq!(
        batch.statistics.expect("statistics").assigned_population(),
        2
    );
}

#[test]
fn session_poll_keeps_statistics_separate_from_capacity_header() {
    let mut script = Script::once(200, STATISTICS_ONLY);
    let Poll::Batch(batch) = poll(
        &mut script,
        QUEUE,
        0,
        11,
        "queue-token-canary",
        &RefreshGate::new(),
        || Ok(()),
    )
    .expect("poll session") else {
        panic!("statistics-only poll must produce a batch");
    };
    assert_eq!(batch.jobs, Vec::<InnerJob>::new());
    assert_eq!(
        batch.statistics.expect("statistics").assigned_population(),
        2
    );
    assert_eq!(script.seen.len(), 1);
    assert_eq!(script.seen[0].method, Method::Get);
    assert_eq!(header(&script.seen[0], CAPACITY_HEADER), Some("11"));
}

#[test]
fn session_poll_keeps_transport_failure_uncertain() {
    let mut script = Script::fail(TransportFail::Reset);
    let error = poll(
        &mut script,
        QUEUE,
        0,
        11,
        "queue-token-canary",
        &RefreshGate::new(),
        || Ok(()),
    )
    .expect_err("reset poll");
    assert_eq!(error.certainty(), Certainty::Uncertain);
    assert_eq!(script.seen.len(), 1);
}

#[test]
fn omitted_null_and_empty_body_decode_the_same() {
    let omitted = envelope(None);
    let null = envelope(Some("null"));
    let empty = envelope(Some("\"\""));
    let omitted = parse_poll(200, &omitted).expect("omitted body");
    let null = parse_poll(200, &null).expect("null body");
    let empty = parse_poll(200, &empty).expect("empty body");
    assert_eq!(omitted, null);
    assert_eq!(null, empty);
}

#[test]
fn body_accepts_only_string_or_null() {
    for body in ["0", "false", "[]", "{}"] {
        let encoded = envelope(Some(body));
        assert_eq!(parse_poll(200, &encoded), Err(WireError::Malformed));
    }
}

#[test]
fn invalid_envelope_identity_and_limits_stay_rejected() {
    assert_eq!(
        parse_poll(
            200,
            r#"{"messageType":"RunnerScaleSetJobMessages","body":"[]"}"#
        ),
        Err(WireError::Malformed)
    );
    assert_eq!(
        parse_poll(200, r#"{"messageId":1,"body":"[]"}"#),
        Err(WireError::Malformed)
    );
    assert_eq!(
        parse_poll(
            200,
            r#"{"messageId":-1,"messageType":"RunnerScaleSetJobMessages","body":"[]"}"#
        ),
        Err(WireError::Malformed)
    );
    assert_eq!(
        parse_poll(200, r#"{"messageId":1,"messageType":"other","body":"[]"}"#),
        Err(WireError::UnsupportedEnvelope)
    );
    let oversized = format!(
        r#"{{"messageId":1,"messageType":"RunnerScaleSetJobMessages","body":"{}"}}"#,
        "x".repeat(MAX_POLL_BODY_BYTES + 1)
    );
    assert_eq!(parse_poll(200, &oversized), Err(WireError::Malformed));
}

#[test]
fn nonempty_valid_job_batch_stays_unchanged() {
    let body = format!(
        r#"{{"messageId":3,"messageType":"RunnerScaleSetJobMessages",{STATISTICS},"body":"[{{\"messageType\":\"JobAvailable\",\"runnerRequestId\":41}}]"}}"#
    );
    let Poll::Batch(batch) = parse_poll(200, &body).expect("valid job batch") else {
        panic!("valid job batch must produce a batch");
    };
    assert_eq!(batch.message_id, 3);
    assert_eq!(batch.jobs.len(), 1);
    assert_eq!(batch.jobs[0].request_id, Some(41));
    assert_eq!(
        batch.statistics.expect("statistics").assigned_population(),
        2
    );
}

fn envelope(body: Option<&str>) -> String {
    let body = body.map_or_else(String::new, |value| format!(",\"body\":{value}"));
    format!(r#"{{"messageId":2,"messageType":"RunnerScaleSetJobMessages",{STATISTICS}{body}}}"#)
}
