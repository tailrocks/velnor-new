// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/tests/broker_protocol.rs

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::json;
use velnor_model::RUNNER_PROTOCOL_VERSION;
use velnor_runner::protocol::{
    AcquireJobOutcome, BrokerClient, BrokerErrorCategory, BrokerPoll, RUNNER_JOB_REQUEST,
    RetryDelays, RunServiceClient, RunnerOs, RunnerStatus, ServiceErrorKind, TaskAgentMessage,
    TaskAgentSession, acquire_job, broker_error_category, service_error_kind,
};

use super::support::{
    JOB_ID, PLAN_ID, RecordedRequest, Reply, TOKEN, TestServer, assert_bearer,
    assert_github_status, complete_job,
};

const UPSTREAM_OS: &str = if cfg!(target_os = "linux") {
    "Linux"
} else if cfg!(target_os = "macos") {
    "macOS"
} else {
    "Windows"
};
const UPSTREAM_ARCHITECTURE: &str = if cfg!(target_arch = "x86_64") {
    "X64"
} else if cfg!(target_arch = "aarch64") {
    "ARM64"
} else if cfg!(target_arch = "x86") {
    "X86"
} else {
    "ARM"
};

fn mount_broker_routes(server: &TestServer, run_service_url: &str) -> Result<()> {
    server.respond("POST", "/broker/session", |_| {
        Reply::json(
            200,
            &json!({
                "sessionId": "session-1",
                "ownerName": "host (PID: 1)",
                "agent": {
                    "id": 42,
                    "name": "velnor-test",
                    "version": RUNNER_PROTOCOL_VERSION,
                    "osDescription": std::env::consts::OS
                },
                "useFipsEncryption": false
            }),
        )
    })?;
    let busy_message = serde_json::to_value(job_message(run_service_url))?;
    server.respond("GET", "/broker/message", move |request| {
        match request.query("status") {
            Some("Online") => Reply::status(204),
            Some("Busy") => Reply::json(200, &busy_message),
            _ => Reply::status(400),
        }
    })
}

fn mount_run_service_routes(server: &TestServer) -> Result<()> {
    server.respond("POST", "/run/jobs/123/acquirejob", |_| {
        Reply::json(
            200,
            &json!({"planId": PLAN_ID, "jobId": JOB_ID, "jobName": "test"}),
        )
    })?;
    server.respond("POST", "/run/jobs/123/completejob", |_| Reply::status(204))
}

fn assert_poll_query(request: &RecordedRequest, status: &str, disable_update: &str) {
    assert_bearer(request);
    assert_eq!(request.query("sessionId"), Some("session-1"));
    assert_eq!(request.query("status"), Some(status));
    assert_eq!(
        request.query("runnerVersion"),
        Some(RUNNER_PROTOCOL_VERSION)
    );
    assert_eq!(request.query("os"), Some(UPSTREAM_OS));
    assert_eq!(request.query("architecture"), Some(UPSTREAM_ARCHITECTURE));
    assert_eq!(request.query("disableUpdate"), Some(disable_update));
}

fn assert_recorded_bodies(server: &TestServer) -> Result<()> {
    let session = server.only_request("POST", "/broker/session")?;
    assert_bearer(&session);
    let session_body = session.json()?;
    assert_eq!(session_body["ownerName"], "host (PID: 1)");
    assert_eq!(session_body["agent"]["id"], 42);
    assert_eq!(session_body["agent"]["name"], "velnor-test");
    assert_eq!(session_body["brokerSession"], true);
    assert!(session_body.get("useFipsEncryption").is_none());

    let polls = server.requests_to("GET", "/broker/message")?;
    assert_eq!(polls.len(), 2);
    assert_poll_query(&polls[0], "Online", "true");
    assert_poll_query(&polls[1], "Busy", "false");

    let acquire = server.only_request("POST", "/run/jobs/123/acquirejob")?;
    assert_bearer(&acquire);
    let acquire_body = acquire.json()?;
    assert_eq!(acquire_body["jobMessageId"], "broker-message");
    assert_eq!(acquire_body["runnerOS"], "Linux");
    assert_eq!(acquire_body["billingOwnerId"], "42");

    let complete_body = server
        .only_request("POST", "/run/jobs/123/completejob")?
        .json()?;
    assert_eq!(complete_body["planId"], PLAN_ID);
    assert_eq!(complete_body["jobId"], JOB_ID);
    assert_eq!(complete_body["conclusion"], "succeeded");
    Ok(())
}

#[tokio::test]
async fn broker_run_service_happy_path_acquires_and_completes_job() -> Result<()> {
    let server = TestServer::start().await?;
    let broker = BrokerClient::new(
        &format!("{}/broker", server.uri()),
        RunnerOs::detect()?,
        TOKEN,
    )?;
    let run_service = RunServiceClient::new(TOKEN)?;
    let run_service_url = format!("{}/run/jobs/123", server.uri());
    mount_broker_routes(&server, &run_service_url)?;
    mount_run_service_routes(&server)?;

    let session = broker
        .create_session(&TaskAgentSession::new("host (PID: 1)", 42, "velnor-test"))
        .await?;
    assert_eq!(session.session_id.as_deref(), Some("session-1"));

    let BrokerPoll {
        status,
        message: idle,
    } = broker
        .get_runner_message("session-1", RunnerStatus::Online, true)
        .await?;
    assert_eq!(status, 204);
    assert!(idle.is_none());

    let poll = broker
        .get_runner_message("session-1", RunnerStatus::Busy, false)
        .await?;
    let message = poll.message.context("job message")?;
    assert_eq!(message.message_id, 99);
    assert_eq!(message.message_type, RUNNER_JOB_REQUEST);

    let acquired = acquire_job(
        RetryDelays::IMMEDIATE,
        &run_service_url,
        "broker-message",
        "Linux",
        Some("42"),
        fresh_token,
    )
    .await?;
    let AcquireJobOutcome::Acquired(job) = acquired else {
        bail!("expected acquired job");
    };
    assert_eq!(job.raw.expose()["planId"], PLAN_ID);
    assert_eq!(job.raw.expose()["jobId"], JOB_ID);

    run_service
        .complete_job(&run_service_url, &complete_job())
        .await?;

    assert_recorded_bodies(&server)
}

#[tokio::test]
async fn broker_poll_auth_failure_is_error_not_idle() -> Result<()> {
    let server = TestServer::start().await?;
    let broker = BrokerClient::new(
        &format!("{}/broker", server.uri()),
        RunnerOs::detect()?,
        TOKEN,
    )?;

    for status in [401, 403] {
        server.reset()?;
        server.respond("GET", "/broker/message", move |_| Reply::status(status))?;

        let Err(error) = broker
            .get_runner_message("session-1", RunnerStatus::Online, true)
            .await
        else {
            bail!("auth poll failure must be an error");
        };
        assert_github_status(&error, status, "get broker message")?;
        assert_eq!(server.requests_to("GET", "/broker/message")?.len(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn broker_poll_and_acknowledge_send_the_injected_os() -> Result<()> {
    let server = TestServer::start().await?;
    let injected = if cfg!(target_os = "linux") {
        RunnerOs::MacOs
    } else {
        RunnerOs::Linux
    };
    assert_ne!(injected.as_str(), UPSTREAM_OS);
    let broker = BrokerClient::new(&format!("{}/broker", server.uri()), injected, TOKEN)?;
    server.respond("GET", "/broker/message", |_| Reply::status(204))?;
    server.respond("POST", "/broker/acknowledge", |_| Reply::status(200))?;

    broker
        .get_runner_message("session-1", RunnerStatus::Online, true)
        .await?;
    broker
        .acknowledge_runner_request("session-1", "request-1", RunnerStatus::Busy)
        .await?;

    let poll = server.only_request("GET", "/broker/message")?;
    let acknowledge = server.only_request("POST", "/broker/acknowledge")?;
    assert_eq!(poll.query("os"), Some(injected.as_str()));
    assert_eq!(acknowledge.query("os"), Some(injected.as_str()));
    Ok(())
}

#[tokio::test]
async fn broker_acknowledgement_gives_up_after_five_seconds() -> Result<()> {
    let server = TestServer::start().await?;
    let broker = BrokerClient::new(
        &format!("{}/broker", server.uri()),
        RunnerOs::detect()?,
        TOKEN,
    )?;
    server.respond("POST", "/broker/acknowledge", |_| Reply::stall())?;

    let started = Instant::now();
    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        broker.acknowledge_runner_request("session-1", "request-1", RunnerStatus::Online),
    )
    .await
    .context("acknowledgement must give up within ten seconds")?;
    assert!(
        started.elapsed() < Duration::from_millis(6500),
        "acknowledgement took {:?}",
        started.elapsed()
    );

    let error = outcome.err().context("a stalled acknowledgement fails")?;
    assert!(format!("{error:#}").contains("timed out"), "{error:#}");
    let request = server.only_request("POST", "/broker/acknowledge")?;
    assert_eq!(request.json()?["runnerRequestId"], "request-1");
    Ok(())
}

async fn fresh_token() -> Result<String> {
    Ok(TOKEN.to_owned())
}

fn job_message(run_service_url: &str) -> TaskAgentMessage {
    TaskAgentMessage {
        message_id: 99,
        message_type: RUNNER_JOB_REQUEST.to_owned(),
        body: json!({
            "id": "broker-message",
            "runner_request_id": "request-1",
            "should_acknowledge": true,
            "run_service_url": run_service_url,
            "billing_owner_id": "42"
        })
        .to_string(),
    }
}

#[tokio::test]
async fn broker_session_creation_classifies_refusals_without_retrying() -> Result<()> {
    let server = TestServer::start().await?;
    let broker = BrokerClient::new(&format!("{}/broker", server.uri()), TOKEN)?;
    let session = TaskAgentSession::new("host (PID: 1)", 42, "velnor-test");

    for (status, category) in [
        (409, BrokerErrorCategory::Conflict),
        (403, BrokerErrorCategory::Terminal),
        (400, BrokerErrorCategory::Terminal),
        (500, BrokerErrorCategory::Transient),
        (429, BrokerErrorCategory::Transient),
    ] {
        server.reset()?;
        server.respond("POST", "/broker/session", move |_| Reply::status(status))?;

        let error = broker
            .create_session(&session)
            .await
            .err()
            .with_context(|| format!("{status}: session creation must fail"))?;

        assert_github_status(&error, status, "create broker session")?;
        assert_eq!(broker_error_category(&error), Some(category), "{status}");
        assert_eq!(server.requests_to("POST", "/broker/session")?.len(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn broker_session_deletion_names_the_session_in_the_query() -> Result<()> {
    let server = TestServer::start().await?;
    let broker = BrokerClient::new(&format!("{}/broker", server.uri()), TOKEN)?;
    server.respond("DELETE", "/broker/session", |_| Reply::status(204))?;

    broker.delete_session("session-1").await?;

    let request = server.only_request("DELETE", "/broker/session")?;
    assert_bearer(&request);
    assert_eq!(request.query("sessionId"), Some("session-1"));

    server.reset()?;
    server.respond("DELETE", "/broker/session", |_| Reply::status(500))?;
    let error = broker
        .delete_session("session-1")
        .await
        .err()
        .context("a refused deletion fails")?;
    assert_github_status(&error, 500, "delete broker session")?;
    Ok(())
}

fn broker_error(source: &str, error_kind: &str) -> serde_json::Value {
    json!({
        "source": source,
        "errorKind": error_kind,
        "statusCode": 404,
        "errorMessage": "refused"
    })
}

fn typed_broker_error(error_kind: &str) -> serde_json::Value {
    broker_error("actions-broker-listener", error_kind)
}

type RefusalCase = (
    &'static str,
    u16,
    serde_json::Value,
    Option<ServiceErrorKind>,
    Option<ServiceErrorKind>,
);

fn refusal_cases() -> Vec<RefusalCase> {
    vec![
        (
            "RunnerNotFound",
            404,
            typed_broker_error("RunnerNotFound"),
            Some(ServiceErrorKind::RunnerNotFound),
            Some(ServiceErrorKind::RunnerNotFound),
        ),
        (
            "RunnerVersionTooOld",
            403,
            typed_broker_error("RunnerVersionTooOld"),
            Some(ServiceErrorKind::RunnerVersionTooOld),
            None,
        ),
        (
            "HostedRunnerDeprovisioned",
            410,
            typed_broker_error("HostedRunnerDeprovisioned"),
            Some(ServiceErrorKind::HostedRunnerDeprovisioned),
            None,
        ),
        (
            "RunnerSessionInvalid",
            401,
            typed_broker_error("RunnerSessionInvalid"),
            Some(ServiceErrorKind::RunnerSessionInvalid),
            None,
        ),
        (
            "AcknowledgeJobNotFound",
            404,
            typed_broker_error("AcknowledgeJobNotFound"),
            None,
            Some(ServiceErrorKind::AcknowledgeJobNotFound),
        ),
        (
            "another source",
            404,
            broker_error("some-other-service", "RunnerNotFound"),
            None,
            None,
        ),
        (
            "no source",
            404,
            json!({"errorKind": "RunnerNotFound"}),
            None,
            None,
        ),
        (
            "unknown kind",
            404,
            typed_broker_error("SomethingNew"),
            None,
            None,
        ),
    ]
}

#[tokio::test]
async fn broker_refusals_carry_the_typed_error_kind() -> Result<()> {
    let server = TestServer::start().await?;
    let broker = BrokerClient::new(&format!("{}/broker", server.uri()), TOKEN)?;

    for (name, status, body, poll_kind, ack_kind) in refusal_cases() {
        server.reset()?;
        server.respond("GET", "/broker/message", {
            let body = body.clone();
            move |_| Reply::json(status, &body)
        })?;
        server.respond("POST", "/broker/acknowledge", move |_| {
            Reply::json(status, &body)
        })?;

        let polled = broker
            .get_runner_message("session-1", RunnerStatus::Online, true)
            .await
            .err()
            .with_context(|| format!("{name}: the poll must fail"))?;
        assert_github_status(&polled, status, "get broker message")?;
        assert_eq!(service_error_kind(&polled), poll_kind, "{name} poll");

        let acknowledged = broker
            .acknowledge_runner_request("session-1", "request-1", RunnerStatus::Busy)
            .await
            .err()
            .with_context(|| format!("{name}: the acknowledgement must fail"))?;
        assert_github_status(&acknowledged, status, "acknowledge broker runner request")?;
        assert_eq!(service_error_kind(&acknowledged), ack_kind, "{name} ack");
    }
    Ok(())
}
