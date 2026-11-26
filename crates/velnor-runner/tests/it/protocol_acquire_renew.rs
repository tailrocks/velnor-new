// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result, bail};
use reqwest::StatusCode;
use serde_json::{Value, json};
use velnor_runner::protocol::{
    AcquireJobOutcome, AcquireJobSkipReason, BrokerErrorCategory, GitHubApiError,
    RetryDelays, acquire_job, is_transient_acquire_error, renew_failure_is_job_gone,
};

use super::support::{
    JOB_ID, PLAN_ID, Reply, Scripted, TOKEN, TestServer, assert_bearer, foreign_source_error,
    immediate_run_service, run_service_error, run_service_url, scripted,
};

const ACQUIRE_PATH: &str = "/run/jobs/123/acquirejob";
const RENEW_PATH: &str = "/run/jobs/123/renewjob";
const UPSTREAM_ATTEMPTS: usize = 5;

async fn acquire(server: &TestServer, os: &str) -> Result<AcquireJobOutcome> {
    acquire_job(
        RetryDelays::IMMEDIATE,
        &run_service_url(server),
        "broker-message",
        os,
        None,
        || async { Ok(TOKEN.to_owned()) },
    )
    .await
}

fn acquired_job() -> Value {
    json!({"planId": PLAN_ID, "jobId": JOB_ID, "jobName": "test"})
}

#[tokio::test]
async fn acquire_job_retries_failures_and_then_acquires() -> Result<()> {
    let server = TestServer::start().await?;
    server.respond(
        "POST",
        ACQUIRE_PATH,
        scripted(vec![
            Scripted::Text(500, "temporary"),
            Scripted::Text(500, "temporary"),
            Scripted::Json(200, acquired_job()),
        ]),
    )?;

    let outcome = acquire(&server, "Linux")
        .await?;

    let AcquireJobOutcome::Acquired(job) = outcome else {
        bail!("expected an acquired job");
    };
    assert_eq!(job.raw.expose()["jobId"], JOB_ID);
    assert_eq!(server.requests_to("POST", ACQUIRE_PATH)?.len(), 3);
    Ok(())
}

#[tokio::test]
async fn acquire_job_gives_up_as_transient_after_five_attempts() -> Result<()> {
    let server = TestServer::start().await?;
    for (name, script) in [
        ("persistent 500", vec![Scripted::Text(500, "temporary")]),
        (
            "a 401 is retried",
            vec![Scripted::Text(401, "unauthorized")],
        ),
        (
            "a 404 from another source",
            vec![Scripted::Json(404, foreign_source_error(404))],
        ),
        (
            "a 404 without a source",
            vec![Scripted::Json(404, json!({"statusCode": 404}))],
        ),
        ("a plain-text 404", vec![Scripted::Text(404, "not found")]),
        (
            "a 409 from another source",
            vec![Scripted::Json(409, foreign_source_error(409))],
        ),
        ("a dropped connection", vec![Scripted::Hangup]),
        ("a 204 without a body", vec![Scripted::Text(204, "")]),
        ("a 200 with an empty body", vec![Scripted::Text(200, "")]),
        (
            "a 200 with a plain-text body",
            vec![Scripted::Text(200, "ok")],
        ),
        (
            "a 200 with a JSON array",
            vec![Scripted::Json(200, json!([]))],
        ),
        (
            "a 200 with a JSON null",
            vec![Scripted::Json(200, json!(null))],
        ),
        (
            "a 200 JSON object that fails the typed parse",
            vec![Scripted::Json(200, json!({"steps": "not-a-list"}))],
        ),
    ] {
        server.reset()?;
        server.respond("POST", ACQUIRE_PATH, scripted(script))?;

        let outcome = acquire(&server, "Linux")
            .await;

        let error = outcome
            .err()
            .with_context(|| format!("{name}: must fail"))?;
        assert!(is_transient_acquire_error(&error), "{name}: {error:#}");
        assert_eq!(
            server.requests_to("POST", ACQUIRE_PATH)?.len(),
            UPSTREAM_ATTEMPTS,
            "{name}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn acquire_job_typed_parse_failure_reports_position_and_path_without_the_value() -> Result<()>
{
    const SENTINEL: &str = "zq-sentinel-9d2f";
    let server = TestServer::start().await?;
    server.respond(
        "POST",
        ACQUIRE_PATH,
        scripted(vec![Scripted::Json(
            200,
            json!({"planId": PLAN_ID, "jobId": JOB_ID, "requestId": SENTINEL}),
        )]),
    )?;

    let error = acquire(&server, "Linux")
        .await
        .err()
        .context("a wrong-typed field must fail the acquire")?;

    assert!(is_transient_acquire_error(&error), "{error:#}");
    let rendered = format!("{error:#}");
    assert!(!rendered.contains(SENTINEL), "{rendered}");
    let api_error = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<GitHubApiError>())
        .context("the chain must carry the GitHubApiError")?;
    assert!(!api_error.body.contains(SENTINEL), "{}", api_error.body);
    assert!(api_error.body.contains("requestId"), "{}", api_error.body);
    assert!(
        api_error.body.contains("line 1 column"),
        "{}",
        api_error.body
    );
    Ok(())
}

#[tokio::test]
async fn acquire_job_classifies_typed_non_retriable_statuses() -> Result<()> {
    let server = TestServer::start().await?;

    for (status, reason, expected_category, wire_code) in [
        (
            StatusCode::BAD_REQUEST,
            AcquireJobSkipReason::NotFound,
            BrokerErrorCategory::Terminal,
            json!(404),
        ),
        (
            StatusCode::NOT_FOUND,
            AcquireJobSkipReason::AlreadyAcquired,
            BrokerErrorCategory::Conflict,
            json!(409),
        ),
        (
            StatusCode::NOT_FOUND,
            AcquireJobSkipReason::NotFound,
            BrokerErrorCategory::Terminal,
            json!(404),
        ),
        (
            StatusCode::CONFLICT,
            AcquireJobSkipReason::AlreadyAcquired,
            BrokerErrorCategory::Conflict,
            json!(409),
        ),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            AcquireJobSkipReason::Unprocessable,
            BrokerErrorCategory::Conflict,
            json!(422),
        ),
        (
            StatusCode::NOT_FOUND,
            AcquireJobSkipReason::NotFound,
            BrokerErrorCategory::Terminal,
            json!("404"),
        ),
    ] {
        server.reset()?;
        let refusal = json!({
            "source": "actions-run-service",
            "statusCode": wire_code,
            "errorMessage": "acquire refused"
        });
        let reply_body = refusal.clone();
        server.respond("POST", ACQUIRE_PATH, move |_| {
            Reply::json(status.as_u16(), &reply_body)
        })?;

        let outcome = acquire(&server, std::env::consts::OS)
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
        assert_eq!(server.requests_to("POST", ACQUIRE_PATH)?.len(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn renew_job_posts_the_plan_and_job_to_renewjob() -> Result<()> {
    let server = TestServer::start().await?;
    server.respond("POST", RENEW_PATH, |_| {
        Reply::json(200, &json!({"lockedUntil": "2026-11-27T12:00:00Z"}))
    })?;

    let renewed = immediate_run_service()?
        .renew_job(&run_service_url(&server), PLAN_ID, JOB_ID)
        .await?;

    assert_eq!(renewed.locked_until, "2026-11-27T12:00:00Z");
    let request = server.only_request("POST", RENEW_PATH)?;
    assert_bearer(&request);
    assert_eq!(request.json()?, json!({"planId": PLAN_ID, "jobId": JOB_ID}));
    Ok(())
}

#[tokio::test]
async fn renew_job_reports_gone_only_for_a_typed_404() -> Result<()> {
    let server = TestServer::start().await?;
    let run_service = immediate_run_service()?;
    let token_bearing_404 = json!({
        "source": "actions-run-service",
        "statusCode": 404,
        "errorMessage": "token for the job was not found"
    });

    for (name, script, requests, gone) in [
        (
            "typed 404",
            vec![Scripted::Json(404, run_service_error(404))],
            1,
            true,
        ),
        (
            "typed 404 whose message the body sanitizer would redact",
            vec![Scripted::Json(404, token_bearing_404)],
            1,
            true,
        ),
        (
            "typed 404 body under HTTP 400",
            vec![Scripted::Json(400, run_service_error(404))],
            1,
            true,
        ),
        (
            "typed 409 body under HTTP 404",
            vec![Scripted::Json(404, run_service_error(409))],
            UPSTREAM_ATTEMPTS,
            false,
        ),
        (
            "persistent 500",
            vec![Scripted::Text(500, "temporary")],
            UPSTREAM_ATTEMPTS,
            false,
        ),
        (
            "404 from another source",
            vec![Scripted::Json(404, foreign_source_error(404))],
            UPSTREAM_ATTEMPTS,
            false,
        ),
        (
            "typed 409",
            vec![Scripted::Json(409, run_service_error(409))],
            UPSTREAM_ATTEMPTS,
            false,
        ),
    ] {
        server.reset()?;
        server.respond("POST", RENEW_PATH, scripted(script))?;

        let outcome = run_service
            .renew_job(&run_service_url(&server), PLAN_ID, JOB_ID)
            .await;

        let error = outcome
            .err()
            .with_context(|| format!("{name}: must fail"))?;
        assert_eq!(renew_failure_is_job_gone(&error), gone, "{name}: {error:#}");
        assert_eq!(
            server.requests_to("POST", RENEW_PATH)?.len(),
            requests,
            "{name}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn renew_job_retries_a_transient_failure_and_then_renews() -> Result<()> {
    let server = TestServer::start().await?;
    server.respond(
        "POST",
        RENEW_PATH,
        scripted(vec![
            Scripted::Text(500, "temporary"),
            Scripted::Json(200, json!({"lockedUntil": "2026-11-27T12:00:00Z"})),
        ]),
    )?;

    let renewed = immediate_run_service()?
        .renew_job(&run_service_url(&server), PLAN_ID, JOB_ID)
        .await?;

    assert_eq!(renewed.locked_until, "2026-11-27T12:00:00Z");
    assert_eq!(server.requests_to("POST", RENEW_PATH)?.len(), 2);
    Ok(())
}
