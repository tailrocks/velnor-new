// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Method;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::json;
use url::Url;
use velnor_runner::context_value::ContextValue;
use velnor_runner::exec::{AdapterRegistry, JobCancel};
use velnor_runner::expr::{ExprEnv, RootContext, evaluate_expression};
use velnor_runner::job_message::{AgentJobRequestMessage, ContainerResource};
use velnor_runner::lifecycle::RunJitOptions;
use velnor_runner::protocol::{
    BrokerClient, DecodedJitConfig, DecodedJitCredentials, DecodedJitRunnerSettings,
    FeedStreamClient, GitHubApiError, GithubHttpResponse, OAuthAccessToken, OAuthJwtCredentials,
    RetryDelays, RunServiceClient, RunServiceVariableValue, RunnerOs, SessionRecovery,
    TaskAgentMessage, TaskAgentSession, TwirpResultsClient, acquire_job, github_json_http_response,
};
use velnor_runner::secret::Secret;

use super::support::{Reply, TOKEN, TestServer, complete_job};

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
    let broker = BrokerClient::new(
        "https://broker.actions.githubusercontent.com/",
        RunnerOs::Linux,
        BEARER,
    )?;
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

#[test]
fn expression_environment_debug_output_contains_no_context_value() -> Result<()> {
    let message = AgentJobRequestMessage::parse_json(FIXTURE).context("parse fixture")?;
    let values = message.materialize_context_values()?;
    let mut env = ExprEnv::new(std::path::PathBuf::from("/workspace"));
    env.set_context(
        RootContext::Github,
        values.expose().get("github").context("github context")?,
    )?;
    env.set_context(
        RootContext::Secrets,
        values.expose().get("secrets").context("secrets context")?,
    )?;
    assert!(evaluate_expression("github.token", &env)?.is_truthy());
    for format in [format!("{env:?}"), format!("{env:#?}")] {
        assert_no_secret("ExprEnv", &format);
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

    let outcome = acquire_job(
        RetryDelays::IMMEDIATE,
        &format!("{}/run/jobs/1", server.uri()),
        "message",
        "Linux",
        None,
        || async { Ok(BEARER.to_owned()) },
    )
    .await?;
    assert_no_secret("AcquireJobOutcome", &format!("{outcome:?}"));
    Ok(())
}

const SENTINEL: &str = "velnor-sentinel-value-0042";

type Rendered = Vec<(&'static str, String)>;

fn secret_bearing_values() -> Result<Rendered> {
    let mut values = credential_values()?;
    values.extend(protocol_values()?);
    Ok(values)
}

fn jit_config() -> DecodedJitConfig {
    DecodedJitConfig {
        settings: DecodedJitRunnerSettings {
            agent_id: Some(1),
            agent_name: None,
            pool_id: None,
            pool_name: None,
            server_url: None,
            server_url_v2: None,
            github_url: None,
            work_folder: None,
            use_v2_flow: true,
            ephemeral: true,
            disable_update: true,
            session_recovery: SessionRecovery::default(),
        },
        credentials: DecodedJitCredentials {
            scheme: "OAuth".to_owned(),
            data: [("clientSecret".to_owned(), SENTINEL.to_owned())].into(),
        },
        private_key_pem: SENTINEL.to_owned(),
    }
}

fn sentinel_url() -> Result<Url> {
    let mut url = Url::parse("https://token.example.test/path")?;
    url.set_username("user")
        .map_err(|()| anyhow::anyhow!("set username"))?;
    url.set_password(Some(SENTINEL))
        .map_err(|()| anyhow::anyhow!("set password"))?;
    url.set_query(Some(&format!("sig={SENTINEL}")));
    url.set_fragment(Some(SENTINEL));
    Ok(url)
}

fn credential_values() -> Result<Rendered> {
    let jit = jit_config();
    let session = TaskAgentSession::new("host", 1, "runner");
    let token = OAuthAccessToken {
        token: SENTINEL.to_owned(),
        expires_in: Some(Duration::from_secs(60)),
    };
    let jwt = OAuthJwtCredentials {
        client_id: "client".to_owned(),
        authorization_url: sentinel_url()?.to_string(),
        token_url: sentinel_url()?.to_string(),
        private_key_pem: SENTINEL.to_owned(),
    };
    let variable = RunServiceVariableValue {
        value: SENTINEL.to_owned(),
        is_secret: true,
    };
    Ok(vec![
        ("OAuthAccessToken", format!("{token:?}")),
        ("RunServiceVariableValue", format!("{variable:?}")),
        ("TaskAgentSession", format!("{session:#?}")),
        ("DecodedJitConfig", format!("{jit:#?}")),
        ("DecodedJitCredentials", format!("{:#?}", jit.credentials)),
        ("OAuthJwtCredentials", format!("{jwt:?}")),
    ])
}

fn protocol_values() -> Result<Rendered> {
    let mut headers = HeaderMap::new();
    headers.insert("x-sentinel", HeaderValue::from_static(SENTINEL));
    let container: ContainerResource = serde_json::from_value(json!({
        "alias": "service",
        "properties": {"image": "example", "env": {"VALUE": SENTINEL}}
    }))?;
    let api_error = GitHubApiError {
        status: 401,
        action: "action".to_owned(),
        body: SENTINEL.to_owned(),
        category: None,
        kind: None,
    };
    let message = TaskAgentMessage {
        message_id: 1,
        message_type: "RunnerJobRequest".to_owned(),
        body: SENTINEL.to_owned(),
    };
    let response = GithubHttpResponse {
        status: 200,
        body: Secret::new(SENTINEL.to_owned()),
        headers: Secret::new(headers),
    };
    Ok(vec![
        ("GitHubApiError", format!("{api_error:?}")),
        ("TaskAgentMessage", format!("{message:?}")),
        ("ContainerResource", format!("{container:#?}")),
        ("GithubHttpResponse", format!("{response:#?}")),
    ])
}

#[test]
fn every_secret_bearing_debug_impl_hides_its_secret() -> Result<()> {
    for (label, rendered) in secret_bearing_values()? {
        assert!(
            !rendered.contains(SENTINEL),
            "{label} Debug output leaks the secret: {rendered}"
        );
    }
    Ok(())
}

#[test]
fn non_secret_run_service_variable_stays_visible_in_debug() {
    let rendered = format!(
        "{:?}",
        RunServiceVariableValue {
            value: "visible-output".to_owned(),
            is_secret: false,
        }
    );
    assert!(rendered.contains("visible-output"), "{rendered}");
}

#[tokio::test]
async fn a_rejected_request_error_never_carries_a_token_from_the_response_body() -> Result<()> {
    let server = TestServer::start().await?;
    let run_service = RunServiceClient::new(TOKEN)?.with_retry_delays(RetryDelays::IMMEDIATE);
    let run_service_url = format!("{}/run/jobs/1", server.uri());

    server.respond("POST", "/run/jobs/1/completejob", |_| {
        Reply::json(
            401,
            &json!({"message": "Bad credentials", "access_token": SENTINEL}),
        )
    })?;
    let leaking = run_service
        .complete_job(&run_service_url, &complete_job())
        .await
        .err()
        .context("a 401 completion fails")?;
    for rendered in [
        format!("{leaking}"),
        format!("{leaking:#}"),
        format!("{leaking:?}"),
        format!("{leaking:#?}"),
    ] {
        assert!(!rendered.contains(SENTINEL), "{rendered}");
        assert!(rendered.contains("401"), "{rendered}");
    }

    server.reset()?;
    server.respond("POST", "/run/jobs/1/completejob", |_| {
        Reply::text(401, "plain failure text")
    })?;
    let plain = run_service
        .complete_job(&run_service_url, &complete_job())
        .await
        .err()
        .context("a 401 completion fails")?;
    assert!(format!("{plain:#}").contains("plain failure text"));
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
        upload_block_budget: Duration::from_secs(60),
        cancel_poll_error_delay: Duration::from_secs(2),
        session_retry_interval: Duration::from_secs(30),
        session_close_timeout: Duration::from_secs(30),
        report_budget: Duration::from_secs(10),
        upload_drain_budget: Duration::from_secs(600),
        feed_flush_interval: Duration::from_millis(250),
        retry_delays: RetryDelays::default(),
        tool_cache: None,
        os: RunnerOs::Linux,
        adapters: AdapterRegistry::empty(),
    };

    assert!(!format!("{options:?}").contains(JIT));
}
