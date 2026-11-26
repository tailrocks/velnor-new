// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Method;
use serde_json::json;
use velnor_runner::context_value::ContextValue;
use velnor_runner::exec::{AdapterRegistry, JobCancel};
use velnor_runner::job_message::AgentJobRequestMessage;
use velnor_runner::lifecycle::RunJitOptions;
use velnor_runner::protocol::{
    BrokerClient, FeedStreamClient, RunServiceClient, TwirpResultsClient, github_json_http_response,
};
use velnor_runner::secret::Secret;

use super::support::{Reply, TestServer};

const FIXTURE: &str = include_str!("../../fixtures/job-message-wire.json");

const BEARER: &str = "bearer-secret-0001";

const SECRETS: &[&str] = &[
    BEARER,
    "token-value",
    "system-token",
    "github-context-token",
    "context-secret-value",
    "velnor-redaction-marker-registry",
];

fn assert_no_secret(label: &str, debug: &str) {
    for secret in SECRETS {
        assert!(
            !debug.contains(secret),
            "{label} Debug output leaks {secret}: {debug}"
        );
    }
}

#[test]
fn client_debug_output_contains_no_bearer_token() -> Result<()> {
    let broker = BrokerClient::new("https://broker.actions.githubusercontent.com/", BEARER)?;
    let run_service = RunServiceClient::new(BEARER)?;
    let feed = FeedStreamClient::new(
        "https://pipelines.actions.githubusercontent.com/feed",
        BEARER,
    )?;
    let results = TwirpResultsClient::new(
        "https://results-receiver.actions.githubusercontent.com/",
        BEARER,
    )?;

    assert_no_secret("BrokerClient", &format!("{broker:?}"));
    assert_no_secret("RunServiceClient", &format!("{run_service:?}"));
    assert_no_secret("FeedStreamClient", &format!("{feed:?}"));
    assert_no_secret("TwirpResultsClient", &format!("{results:?}"));
    Ok(())
}

#[test]
fn parsed_job_message_debug_output_contains_no_secret_value() -> Result<()> {
    let message = AgentJobRequestMessage::parse_json(FIXTURE).context("parse fixture")?;
    for secret in &SECRETS[1..] {
        assert!(FIXTURE.contains(secret), "fixture lacks {secret}");
    }
    let variable = message
        .variables
        .get("system.github.token")
        .and_then(|variable| variable.value.as_ref())
        .context("secret variable")?;
    assert_eq!(variable.expose(), "token-value");
    assert_no_secret("AgentJobRequestMessage", &format!("{message:?}"));
    assert_no_secret("AgentJobRequestMessage", &format!("{message:#?}"));
    Ok(())
}

#[test]
fn materialized_context_values_debug_output_contains_no_secret_value() -> Result<()> {
    let message = AgentJobRequestMessage::parse_json(FIXTURE).context("parse fixture")?;
    let by_name = message.materialize_context_values()?;
    let ordered = message.materialize_context_values_ordered()?;
    let github = by_name.expose().get("github").context("github context")?;
    assert_eq!(
        github.get("token"),
        Some(&ContextValue::String("github-context-token".to_owned()))
    );
    assert!(ordered.expose().iter().any(|(name, _)| name == "secrets"));
    for format in [
        format!("{by_name:?}"),
        format!("{by_name:#?}"),
        format!("{ordered:?}"),
        format!("{ordered:#?}"),
    ] {
        assert_no_secret("materialized context values", &format);
    }
    Ok(())
}

#[tokio::test]
async fn response_and_acquired_job_debug_output_contains_no_secret_value() -> Result<()> {
    let server = TestServer::start().await?;
    let job: serde_json::Value = serde_json::from_str(FIXTURE)?;
    server.respond("POST", "/run/jobs/1/acquirejob", move |_| {
        Reply::json(200, &job)
    })?;
    server.respond("GET", "/echo", |_| {
        Reply::json(200, &json!({"token": "token-value"}))
    })?;

    let response = github_json_http_response(
        Method::GET,
        &format!("{}/echo", server.uri()),
        BEARER,
        None,
        Duration::from_secs(5),
    )
    .await?;
    assert_no_secret("GithubHttpResponse", &format!("{response:?}"));

    let outcome = RunServiceClient::new(BEARER)?
        .acquire_job(
            &format!("{}/run/jobs/1", server.uri()),
            "message",
            "Linux",
            None,
        )
        .await?;
    assert_no_secret("AcquireJobOutcome", &format!("{outcome:?}"));
    Ok(())
}

#[test]
fn run_jit_options_debug_output_hides_the_jit_configuration() {
    const JIT: &str = "jit-config-bundle-0001";
    let options = RunJitOptions {
        jit: Secret::new(JIT.to_owned()),
        work_dir: std::path::PathBuf::from("_work"),
        shutdown: JobCancel::new(),
        renew_interval: Duration::from_secs(60),
        renew_lock_grace: Duration::from_secs(300),
        cancel_timeout_floor: Duration::from_secs(60),
        cancel_poll_error_delay: Duration::from_secs(2),
        adapters: AdapterRegistry::empty(),
    };

    assert!(!format!("{options:?}").contains(JIT));
}
