// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/tests/broker_protocol.rs

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::StatusCode;
use serde_json::{Value, json};
use velnor_runner::protocol::{
    AcquireJobOutcome, AcquireJobSkipReason, BrokerClient, BrokerErrorCategory, BrokerPoll,
    RUNNER_JOB_REQUEST, RUNNER_VERSION, RetryDelays, RunServiceClient, RunServiceCompleteJob,
    RunnerOs, RunnerStatus, TaskAgentMessage, TaskAgentSession, TaskResult, acquire_job,
};

use super::support::{
    RecordedRequest, Reply, TOKEN, TestServer, assert_bearer, assert_github_status,
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

const PLAN_ID: &str = "00000000-0000-0000-0000-000000000002";
const JOB_ID: &str = "00000000-0000-0000-0000-000000000001";

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
                    "version": RUNNER_VERSION,
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
    assert_eq!(request.query("runnerVersion"), Some(RUNNER_VERSION));
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

    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        broker.acknowledge_runner_request("session-1", "request-1", RunnerStatus::Online),
    )
    .await
    .context("acknowledgement must give up within ten seconds")?;

    let error = outcome.err().context("a stalled acknowledgement fails")?;
    assert!(format!("{error:#}").contains("timed out"), "{error:#}");
    let request = server.only_request("POST", "/broker/acknowledge")?;
    assert_eq!(request.json()?["runnerRequestId"], "request-1");
    Ok(())
}

#[tokio::test]
async fn acquire_job_classifies_typed_non_retriable_statuses() -> Result<()> {
    let server = TestServer::start().await?;
    let run_service_url = format!("{}/run/jobs/123", server.uri());

    for (status, reason, expected_category) in [
        (
            StatusCode::NOT_FOUND,
            AcquireJobSkipReason::NotFound,
            BrokerErrorCategory::Terminal,
        ),
        (
            StatusCode::CONFLICT,
            AcquireJobSkipReason::AlreadyAcquired,
            BrokerErrorCategory::Conflict,
        ),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            AcquireJobSkipReason::Unprocessable,
            BrokerErrorCategory::Conflict,
        ),
    ] {
        server.reset()?;
        let refusal = json!({
            "source": "actions-run-service",
            "statusCode": status.as_u16(),
            "errorMessage": "acquire refused"
        });
        let reply_body = refusal.clone();
        server.respond("POST", "/run/jobs/123/acquirejob", move |_| {
            Reply::json(status.as_u16(), &reply_body)
        })?;

        let outcome = acquire_job(
            RetryDelays::IMMEDIATE,
            &run_service_url,
            "broker-message",
            std::env::consts::OS,
            None,
            fresh_token,
        )
        .await?;
        let AcquireJobOutcome::Skipped {
            status: actual,
            body,
            reason: actual_reason,
            category: actual_category,
        } = outcome
        else {
            bail!("expected skipped acquire");
        };
        assert_eq!(actual, status);
        assert_eq!(actual_reason, reason);
        assert_eq!(serde_json::from_str::<Value>(&body)?, refusal);
        assert_eq!(actual_category, expected_category);
        assert_eq!(
            server
                .requests_to("POST", "/run/jobs/123/acquirejob")?
                .len(),
            1
        );
    }
    Ok(())
}

#[tokio::test]
async fn complete_job_retries_5xx_and_succeeds() -> Result<()> {
    let server = TestServer::start().await?;
    let run_service = RunServiceClient::new(TOKEN)?.with_retry_delays(RetryDelays::IMMEDIATE);
    let run_service_url = format!("{}/run/jobs/123", server.uri());
    let attempts = Arc::new(AtomicUsize::new(0));
    let responder_attempts = Arc::clone(&attempts);
    server.respond("POST", "/run/jobs/123/completejob", move |_| {
        if responder_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            Reply::text(500, "temporary")
        } else {
            Reply::status(204)
        }
    })?;

    run_service
        .complete_job(&run_service_url, &complete_job())
        .await?;

    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn complete_job_does_not_retry_non_retryable_4xx() -> Result<()> {
    let server = TestServer::start().await?;
    let run_service = RunServiceClient::new(TOKEN)?;
    let run_service_url = format!("{}/run/jobs/123", server.uri());
    server.respond("POST", "/run/jobs/123/completejob", |_| {
        Reply::text(400, "bad completion")
    })?;

    let Err(error) = run_service
        .complete_job(&run_service_url, &complete_job())
        .await
    else {
        bail!("400 completion must not retry");
    };
    assert_github_status(&error, 400, "complete run-service job")?;

    assert_eq!(
        server
            .requests_to("POST", "/run/jobs/123/completejob")?
            .len(),
        1
    );
    Ok(())
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

async fn fresh_token() -> Result<String> {
    Ok(TOKEN.to_owned())
}

fn complete_job() -> RunServiceCompleteJob {
    RunServiceCompleteJob {
        plan_id: PLAN_ID.to_owned(),
        job_id: JOB_ID.to_owned(),
        conclusion: TaskResult::Succeeded,
        outputs: BTreeMap::new(),
        step_results: Vec::new(),
        annotations: Vec::new(),
        telemetry: Vec::new(),
        environment_url: None,
        billing_owner_id: Some("42".to_owned()),
        infrastructure_failure_category: None,
    }
}
