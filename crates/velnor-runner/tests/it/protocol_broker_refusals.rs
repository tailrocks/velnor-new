// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use serde_json::json;
use velnor_runner::protocol::{
    Acknowledgement, BrokerClient, BrokerErrorCategory, RunnerOs, RunnerStatus, ServiceErrorKind,
    TaskAgentSession, broker_error_category, service_error_kind,
};

use super::support::{Reply, TOKEN, TestServer, assert_bearer, assert_github_status};

#[tokio::test]
async fn broker_session_creation_classifies_refusals_without_retrying() -> Result<()> {
    let server = TestServer::start().await?;
    let broker = BrokerClient::new(&format!("{}/broker", server.uri()), RunnerOs::Linux, TOKEN)?;
    let session = TaskAgentSession::new("host (PID: 1)", 42, "velnor-test");

    for (status, category) in [
        (409, BrokerErrorCategory::Conflict),
        (403, BrokerErrorCategory::Terminal),
        (400, BrokerErrorCategory::Transient),
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
    let broker = BrokerClient::new(&format!("{}/broker", server.uri()), RunnerOs::Linux, TOKEN)?;
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
    let broker = BrokerClient::new(&format!("{}/broker", server.uri()), RunnerOs::Linux, TOKEN)?;

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
            .await;
        if ack_kind == Some(ServiceErrorKind::AcknowledgeJobNotFound) {
            assert_eq!(acknowledged?, Acknowledgement::JobNotFound, "{name} ack");
            continue;
        }
        let acknowledged = acknowledged
            .err()
            .with_context(|| format!("{name}: the acknowledgement must fail"))?;
        assert_github_status(&acknowledged, status, "acknowledge broker runner request")?;
        assert_eq!(service_error_kind(&acknowledged), ack_kind, "{name} ack");
    }
    Ok(())
}
