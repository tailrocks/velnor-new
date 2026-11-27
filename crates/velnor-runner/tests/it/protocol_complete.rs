// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Result, bail};
use serde_json::json;
use velnor_runner::protocol::{CompletionAcknowledgement, RetryDelays, RunServiceClient};

use super::protocol_broker::complete_job;
use super::support::{Reply, TOKEN, TestServer, assert_github_status};

const COMPLETE: &str = "/run/jobs/123/completejob";

async fn mounted() -> Result<(TestServer, RunServiceClient, String)> {
    let server = TestServer::start().await?;
    let client = RunServiceClient::new(TOKEN)?.with_retry_delays(RetryDelays::IMMEDIATE);
    let url = format!("{}/run/jobs/123", server.uri());
    Ok((server, client, url))
}

#[tokio::test]
async fn complete_job_retries_5xx_and_succeeds() -> Result<()> {
    let (server, client, url) = mounted().await?;
    let attempts = Arc::new(AtomicUsize::new(0));
    let responder_attempts = Arc::clone(&attempts);
    server.respond("POST", COMPLETE, move |_| {
        if responder_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            Reply::text(500, "temporary")
        } else {
            Reply::status(204)
        }
    })?;

    client.complete_job(&url, &complete_job()).await?;

    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn complete_job_retries_the_4xx_answers_the_runner_retries() -> Result<()> {
    for status in [400, 403, 409, 422] {
        let (server, client, url) = mounted().await?;
        let attempts = Arc::new(AtomicUsize::new(0));
        let responder_attempts = Arc::clone(&attempts);
        server.respond("POST", COMPLETE, move |_| {
            if responder_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                Reply::text(status, "rejected")
            } else {
                Reply::status(204)
            }
        })?;

        client.complete_job(&url, &complete_job()).await?;

        assert_eq!(attempts.load(Ordering::SeqCst), 2, "status {status}");
    }
    Ok(())
}

#[tokio::test]
async fn complete_job_gives_up_at_once_when_unauthorized() -> Result<()> {
    let (server, client, url) = mounted().await?;
    server.respond("POST", COMPLETE, |_| Reply::text(401, "expired token"))?;

    let Err(error) = client.complete_job(&url, &complete_job()).await else {
        bail!("401 completion must fail");
    };

    assert_github_status(&error, 401, "complete run-service job")?;
    assert_eq!(server.requests_to("POST", COMPLETE)?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn complete_job_stops_after_five_attempts() -> Result<()> {
    let (server, client, url) = mounted().await?;
    server.respond("POST", COMPLETE, |_| Reply::text(400, "bad completion"))?;

    let Err(error) = client.complete_job(&url, &complete_job()).await else {
        bail!("a rejected completion must fail");
    };

    assert_github_status(&error, 400, "complete run-service job")?;
    assert_eq!(server.requests_to("POST", COMPLETE)?.len(), 5);
    Ok(())
}

#[tokio::test]
async fn complete_job_stops_when_the_run_service_reports_the_job_gone() -> Result<()> {
    let (server, client, url) = mounted().await?;
    server.respond("POST", COMPLETE, |_| {
        Reply::json(
            404,
            &json!({"source": "actions-run-service", "statusCode": 404}),
        )
    })?;

    let acknowledgement = client
        .complete_job_with_acknowledgement(&url, &complete_job())
        .await?;

    assert_eq!(
        acknowledgement,
        CompletionAcknowledgement::RemoteObservedTerminal
    );
    assert_eq!(server.requests_to("POST", COMPLETE)?.len(), 1);
    Ok(())
}
