//! Poll envelope statistics tests.

use serde_json::{Value, json};
use velnor_runner_github::{
    CAPACITY_HEADER, Certainty, InnerJob, MAX_POLL_BODY_BYTES, Method, Poll, RefreshGate,
    Statistics, TransportFail, WireError, parse_poll, poll,
};

mod common;

use common::{QUEUE, Script, header};

// External Apache-2.0 fixture: velnor-reference 1b9fc038, SHA-256 edc857a568346af88e2447511c752a55c1730e3971f39f36699c3258cfa76c48.
const STATISTICS_ONLY: &str = include_str!("fixtures/scaleset/message_stats_only.json");
const STATISTICS: &str = r#""statistics":{"totalAcquiredJobs":0,"totalAssignedJobs":2,"totalAvailableJobs":1,"totalBusyRunners":1,"totalIdleRunners":1,"totalRegisteredRunners":2,"totalRunningJobs":1}"#;
const VALID_JOB_BODY: &str = r#"[{"messageType":"JobAvailable","runnerRequestId":41}]"#;

#[test]
fn statistics_only_fixture_uses_assigned_population() {
    let Poll::Batch(batch) = parse_poll(200, STATISTICS_ONLY).expect("valid statistics-only poll")
    else {
        panic!("statistics-only poll must produce a batch");
    };
    assert_eq!(batch.message_id, 2);
    assert_eq!(batch.jobs, Vec::<InnerJob>::new());
    assert_eq!(batch.raw_body, "");
    assert_eq!(batch.statistics, Some(full_statistics()));
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

#[test]
fn assigned_only_statistics_keep_a_valid_job_batch() {
    let statistics = json!({"totalAssignedJobs": 2});
    let body = envelope_with_statistics(Some(&statistics), VALID_JOB_BODY);
    let Poll::Batch(batch) = parse_poll(200, &body).expect("partial statistics and job") else {
        panic!("valid job batch must produce a batch");
    };

    assert_eq!(batch.jobs.len(), 1);
    assert_eq!(batch.jobs[0].request_id, Some(41));
    assert_eq!(
        batch.statistics,
        Some(Statistics {
            total_available_jobs: 0,
            total_acquired_jobs: 0,
            total_assigned_jobs: 2,
            total_running_jobs: 0,
            total_registered_runners: 0,
            total_busy_runners: 0,
            total_idle_runners: 0,
        })
    );
}

#[test]
fn omitted_non_authoritative_counters_default_without_losing_jobs() {
    let full = full_statistics_value();
    for (field, expected_zero) in [
        ("totalAvailableJobs", "total_available_jobs"),
        ("totalAcquiredJobs", "total_acquired_jobs"),
        ("totalRunningJobs", "total_running_jobs"),
        ("totalRegisteredRunners", "total_registered_runners"),
        ("totalBusyRunners", "total_busy_runners"),
        ("totalIdleRunners", "total_idle_runners"),
    ] {
        let mut statistics = full.clone();
        statistics
            .as_object_mut()
            .expect("statistics object")
            .remove(field);
        let body = envelope_with_statistics(Some(&statistics), VALID_JOB_BODY);
        let Poll::Batch(batch) = parse_poll(200, &body).expect("omitted counter") else {
            panic!("valid job batch must produce a batch");
        };

        let mut expected = full_statistics();
        match expected_zero {
            "total_available_jobs" => expected.total_available_jobs = 0,
            "total_acquired_jobs" => expected.total_acquired_jobs = 0,
            "total_running_jobs" => expected.total_running_jobs = 0,
            "total_registered_runners" => expected.total_registered_runners = 0,
            "total_busy_runners" => expected.total_busy_runners = 0,
            "total_idle_runners" => expected.total_idle_runners = 0,
            _ => unreachable!("counter list is exhaustive"),
        }
        assert_eq!(batch.jobs.len(), 1, "{field}");
        assert_eq!(batch.jobs[0].request_id, Some(41), "{field}");
        assert_eq!(batch.statistics, Some(expected), "{field}");
    }
}

#[test]
fn missing_assigned_population_stays_absent() {
    let cases = [
        ("omitted statistics", None),
        ("null statistics", Some(Value::Null)),
        ("empty statistics", Some(json!({}))),
        (
            "assigned counter omitted",
            Some(json!({"totalAvailableJobs": 1})),
        ),
        (
            "assigned counter null",
            Some(json!({"totalAssignedJobs": null})),
        ),
    ];

    for (name, statistics) in cases {
        let body = envelope_with_statistics(statistics.as_ref(), VALID_JOB_BODY);
        let Poll::Batch(batch) = parse_poll(200, &body).expect(name) else {
            panic!("valid job batch must produce a batch: {name}");
        };
        assert_eq!(batch.jobs.len(), 1, "{name}");
        assert_eq!(batch.jobs[0].request_id, Some(41), "{name}");
        assert_eq!(batch.statistics, None, "{name}");
    }
}

#[test]
fn statistics_counters_keep_strict_invalid_type_handling() {
    let fields = [
        "totalAvailableJobs",
        "totalAcquiredJobs",
        "totalAssignedJobs",
        "totalRunningJobs",
        "totalRegisteredRunners",
        "totalBusyRunners",
        "totalIdleRunners",
    ];
    for field in fields {
        let mut statistics = full_statistics_value();
        statistics
            .as_object_mut()
            .expect("statistics object")
            .insert(field.to_owned(), json!("not-an-integer"));
        let body = envelope_with_statistics(Some(&statistics), VALID_JOB_BODY);
        assert_eq!(parse_poll(200, &body), Err(WireError::Malformed), "{field}");

        if field != "totalAssignedJobs" {
            let mut statistics = full_statistics_value();
            statistics
                .as_object_mut()
                .expect("statistics object")
                .insert(field.to_owned(), Value::Null);
            let body = envelope_with_statistics(Some(&statistics), VALID_JOB_BODY);
            assert_eq!(parse_poll(200, &body), Err(WireError::Malformed), "{field}");
        }
    }
}

fn envelope(body: Option<&str>) -> String {
    let body = body.map_or_else(String::new, |value| format!(",\"body\":{value}"));
    format!(r#"{{"messageId":2,"messageType":"RunnerScaleSetJobMessages",{STATISTICS}{body}}}"#)
}

fn envelope_with_statistics(statistics: Option<&Value>, body: &str) -> String {
    let statistics =
        statistics.map_or_else(String::new, |value| format!(",\"statistics\":{value}"));
    let body = Value::String(body.to_owned());
    format!(
        r#"{{"messageId":3,"messageType":"RunnerScaleSetJobMessages"{statistics},"body":{body}}}"#
    )
}

fn full_statistics() -> Statistics {
    Statistics {
        total_available_jobs: 1,
        total_acquired_jobs: 0,
        total_assigned_jobs: 2,
        total_running_jobs: 1,
        total_registered_runners: 2,
        total_busy_runners: 1,
        total_idle_runners: 1,
    }
}

fn full_statistics_value() -> Value {
    json!({
        "totalAvailableJobs": 1,
        "totalAcquiredJobs": 0,
        "totalAssignedJobs": 2,
        "totalRunningJobs": 1,
        "totalRegisteredRunners": 2,
        "totalBusyRunners": 1,
        "totalIdleRunners": 1,
    })
}
