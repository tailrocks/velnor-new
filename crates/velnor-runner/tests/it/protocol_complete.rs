// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result, bail};
use serde_json::json;
use std::time::{Duration, Instant};
use velnor_runner::protocol::{
    BrokerErrorCategory, RetryDelays, RunServiceClient, RunServiceStepResult, ServiceErrorKind,
    TaskResult, TimelineRecordState, broker_error_category, service_error_kind,
};

use super::support::{
    JOB_ID, PLAN_ID, Scripted, TestServer, assert_bearer, assert_github_status, complete_job,
    foreign_source_error, immediate_run_service, run_service_error, run_service_url, scripted,
};

const COMPLETE_PATH: &str = "/run/jobs/123/completejob";
const COMPLETE_ACTION: &str = "complete run-service job";
const UPSTREAM_ATTEMPTS: usize = 5;

enum Expected {
    Accepted,
    JobNotFound(u16),
    Rejected(u16, BrokerErrorCategory),
    Transport,
}

struct CompleteCase {
    name: &'static str,
    script: Vec<Scripted>,
    requests: usize,
    expected: Expected,
}

fn complete_cases() -> Vec<CompleteCase> {
    vec![
        CompleteCase {
            name: "persistent 500 stops after the fifth attempt",
            script: vec![Scripted::Text(500, "temporary")],
            requests: UPSTREAM_ATTEMPTS,
            expected: Expected::Rejected(500, BrokerErrorCategory::Transient),
        },
        CompleteCase {
            name: "500s then success is accepted on the fifth attempt",
            script: vec![
                Scripted::Text(500, "temporary"),
                Scripted::Text(500, "temporary"),
                Scripted::Text(500, "temporary"),
                Scripted::Text(500, "temporary"),
                Scripted::Text(204, ""),
            ],
            requests: UPSTREAM_ATTEMPTS,
            expected: Expected::Accepted,
        },
        CompleteCase {
            name: "a 400 is retried like every other error",
            script: vec![
                Scripted::Text(400, "bad completion"),
                Scripted::Text(400, "bad completion"),
                Scripted::Text(204, ""),
            ],
            requests: 3,
            expected: Expected::Accepted,
        },
        CompleteCase {
            name: "a persistent 400 exhausts the attempts",
            script: vec![Scripted::Text(400, "bad completion")],
            requests: UPSTREAM_ATTEMPTS,
            expected: Expected::Rejected(400, BrokerErrorCategory::Transient),
        },
        CompleteCase {
            name: "a 401 is not retried",
            script: vec![Scripted::Text(401, "unauthorized")],
            requests: 1,
            expected: Expected::Rejected(401, BrokerErrorCategory::Terminal),
        },
        CompleteCase {
            name: "a typed 404 fails the completion as job not found without a retry",
            script: vec![Scripted::Json(404, run_service_error(404))],
            requests: 1,
            expected: Expected::JobNotFound(404),
        },
        CompleteCase {
            name: "the typed body decides, not the HTTP status: 400 with a typed 404",
            script: vec![Scripted::Json(400, run_service_error(404))],
            requests: 1,
            expected: Expected::JobNotFound(400),
        },
        CompleteCase {
            name: "the typed body decides, not the HTTP status: 404 with a typed 409",
            script: vec![Scripted::Json(404, run_service_error(409))],
            requests: UPSTREAM_ATTEMPTS,
            expected: Expected::Rejected(404, BrokerErrorCategory::Transient),
        },
        CompleteCase {
            name: "a 404 from another source is retried",
            script: vec![Scripted::Json(404, foreign_source_error(404))],
            requests: UPSTREAM_ATTEMPTS,
            expected: Expected::Rejected(404, BrokerErrorCategory::Transient),
        },
        CompleteCase {
            name: "a 404 without a source is retried",
            script: vec![Scripted::Text(404, "not found")],
            requests: UPSTREAM_ATTEMPTS,
            expected: Expected::Rejected(404, BrokerErrorCategory::Transient),
        },
        CompleteCase {
            name: "a dropped connection is retried",
            script: vec![Scripted::Hangup],
            requests: UPSTREAM_ATTEMPTS,
            expected: Expected::Transport,
        },
    ]
}

#[tokio::test]
async fn complete_job_follows_the_upstream_retry_policy() -> Result<()> {
    let server = TestServer::start().await?;
    let run_service = immediate_run_service()?;

    for case in complete_cases() {
        server.reset()?;
        server.respond("POST", COMPLETE_PATH, scripted(case.script))?;

        let outcome = run_service
            .complete_job(&run_service_url(&server), &complete_job())
            .await;

        match (case.expected, outcome) {
            (Expected::Accepted, Ok(())) => {}
            (Expected::JobNotFound(status), Err(error)) => {
                assert_github_status(&error, status, COMPLETE_ACTION)
                    .with_context(|| case.name.to_owned())?;
                assert_eq!(
                    service_error_kind(&error),
                    Some(ServiceErrorKind::JobNotFound),
                    "{}",
                    case.name
                );
            }
            (Expected::Rejected(status, category), Err(error)) => {
                assert_github_status(&error, status, COMPLETE_ACTION)
                    .with_context(|| case.name.to_owned())?;
                assert_eq!(
                    broker_error_category(&error),
                    Some(category),
                    "{}",
                    case.name
                );
            }
            (Expected::Transport, Err(error)) => {
                let rendered = format!("{error:#}");
                assert!(
                    rendered.contains("complete run-service job request"),
                    "{}: {rendered}",
                    case.name
                );
            }
            (_, outcome) => bail!("{}: unexpected outcome {outcome:?}", case.name),
        }
        assert_eq!(
            server.requests_to("POST", COMPLETE_PATH)?.len(),
            case.requests,
            "{}",
            case.name
        );
    }
    Ok(())
}

#[tokio::test]
async fn complete_job_resends_the_identical_body_on_every_attempt() -> Result<()> {
    let server = TestServer::start().await?;
    server.respond(
        "POST",
        COMPLETE_PATH,
        scripted(vec![
            Scripted::Text(500, "temporary"),
            Scripted::Text(204, ""),
        ]),
    )?;

    immediate_run_service()?
        .complete_job(&run_service_url(&server), &complete_job())
        .await?;

    let requests = server.requests_to("POST", COMPLETE_PATH)?;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].json()?, requests[1].json()?);
    assert_eq!(requests[1].json()?["planId"], PLAN_ID);
    assert_eq!(requests[1].json()?["jobId"], JOB_ID);
    assert_bearer(&requests[1]);
    Ok(())
}

#[test]
fn default_retry_delays_match_upstream() {
    assert_eq!(
        RetryDelays::default(),
        RetryDelays {
            min: Duration::from_secs(5),
            max: Duration::from_secs(15),
        }
    );
}

#[tokio::test]
async fn complete_job_waits_at_least_the_minimum_delay_between_attempts() -> Result<()> {
    const MIN: Duration = Duration::from_millis(100);
    let server = TestServer::start().await?;
    server.respond(
        "POST",
        COMPLETE_PATH,
        scripted(vec![
            Scripted::Text(500, "temporary"),
            Scripted::Text(500, "temporary"),
            Scripted::Text(204, ""),
        ]),
    )?;
    let run_service =
        RunServiceClient::new("token")?.with_retry_delays(RetryDelays { min: MIN, max: MIN });

    let started = Instant::now();
    run_service
        .complete_job(&run_service_url(&server), &complete_job())
        .await?;

    assert!(started.elapsed() >= MIN * 2, "two gaps of at least {MIN:?}");
    Ok(())
}

fn step_result(name: &str) -> RunServiceStepResult {
    RunServiceStepResult {
        external_id: None,
        number: Some(1),
        name: name.to_owned(),
        action_name: None,
        reference: None,
        kind: None,
        status: TimelineRecordState::Completed,
        conclusion: TaskResult::Succeeded,
        started_at: None,
        completed_at: None,
        completed_log_url: None,
        completed_log_lines: 0,
        annotations: Vec::new(),
        is_background: false,
        background_control_type: None,
        background_control_step_ids: Vec::new(),
    }
}

#[tokio::test]
async fn complete_job_sends_action_identity_log_url_and_background_metadata() -> Result<()> {
    let server = TestServer::start().await?;
    server.respond(
        "POST",
        COMPLETE_PATH,
        scripted(vec![Scripted::Text(204, "")]),
    )?;
    let mut completion = complete_job();
    completion.step_results = vec![
        RunServiceStepResult {
            action_name: Some("actions/checkout".to_owned()),
            reference: Some("v4".to_owned()),
            kind: Some("repository".to_owned()),
            completed_log_url: Some("https://logs.example/1".to_owned()),
            completed_log_lines: 7,
            is_background: true,
            background_control_type: Some("wait".to_owned()),
            background_control_step_ids: vec!["step-0".to_owned()],
            ..step_result("Run actions/checkout@v4")
        },
        step_result("Run echo"),
    ];

    immediate_run_service()?
        .complete_job(&run_service_url(&server), &completion)
        .await?;

    let body = server.only_request("POST", COMPLETE_PATH)?.json()?;
    let first = &body["stepResults"][0];
    assert_eq!(first["action_name"], "actions/checkout");
    assert_eq!(first["ref"], "v4");
    assert_eq!(first["type"], "repository");
    assert_eq!(first["completed_log_url"], "https://logs.example/1");
    assert_eq!(first["completed_log_lines"], 7);
    assert_eq!(first["is_background"], true);
    assert_eq!(first["background_control_type"], "wait");
    assert_eq!(first["background_control_step_ids"], json!(["step-0"]));
    for absent in [
        "action_name",
        "ref",
        "type",
        "completed_log_url",
        "is_background",
        "background_control_type",
        "background_control_step_ids",
    ] {
        assert!(body["stepResults"][1].get(absent).is_none(), "{absent}");
    }
    Ok(())
}

#[tokio::test]
async fn complete_job_serializes_annotation_coordinates_in_camel_case() -> Result<()> {
    let server = TestServer::start().await?;
    server.respond(
        "POST",
        COMPLETE_PATH,
        scripted(vec![Scripted::Text(204, "")]),
    )?;
    let mut completion = complete_job();
    completion.annotations = vec![velnor_runner::protocol::RunServiceAnnotation {
        level: velnor_runner::protocol::RunServiceAnnotationLevel::Failure,
        message: "boom".to_owned(),
        title: None,
        path: Some("a.yml".to_owned()),
        start_line: Some(1),
        end_line: Some(2),
        start_column: Some(3),
        end_column: Some(4),
        step_number: Some(5),
        is_infrastructure_issue: true,
    }];

    immediate_run_service()?
        .complete_job(&run_service_url(&server), &completion)
        .await?;

    let body = server.only_request("POST", COMPLETE_PATH)?.json()?;
    let annotation = &body["annotations"][0];
    for (name, value) in [
        ("startLine", 1),
        ("endLine", 2),
        ("startColumn", 3),
        ("endColumn", 4),
        ("stepNumber", 5),
    ] {
        assert_eq!(annotation[name], value, "{name}");
    }
    assert_eq!(annotation["isInfrastructureIssue"], true);
    Ok(())
}
