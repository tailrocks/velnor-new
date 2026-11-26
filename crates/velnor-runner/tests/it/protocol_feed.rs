// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde_json::Value;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{
    Callback, ErrorResponse, Request, Response,
};
use tokio_tungstenite::{accept_async, accept_hdr_async};
use velnor_runner::protocol::{FeedStreamClient, RUNNER_USER_AGENT};

use super::support::TOKEN;

const PIPELINES_HOST: &str = "pipelines.actions.githubusercontent.com";

#[test]
fn feed_stream_accepts_https_feed_url_from_the_job_message() -> Result<()> {
    FeedStreamClient::new(&format!("https://{PIPELINES_HOST}/_apis/feed"), TOKEN)?;
    FeedStreamClient::new(&format!("wss://{PIPELINES_HOST}/_apis/feed"), TOKEN)?;
    Ok(())
}

#[test]
fn feed_stream_rejects_unapproved_hosts_and_schemes() {
    assert!(FeedStreamClient::new("https://example.com/feed", TOKEN).is_err());
    assert!(FeedStreamClient::new(&format!("ftp://{PIPELINES_HOST}/feed"), TOKEN).is_err());
}

#[tokio::test]
async fn feed_stream_connects_over_websocket_when_given_an_http_url() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut socket = accept_async(stream).await?;
        match socket.next().await.context("feed message")?? {
            Message::Text(text) => Ok::<_, anyhow::Error>(text.to_string()),
            other => bail!("expected a text frame, got {other:?}"),
        }
    });

    let client = FeedStreamClient::new(&format!("http://{address}/feed"), TOKEN)?;
    let mut socket = client.connect().await?;
    FeedStreamClient::send_log_lines(
        &mut socket,
        "step-1",
        vec!["a".to_owned(), "b".to_owned()],
        3,
    )
    .await?;

    let received: Value = serde_json::from_str(&server.await??)?;
    assert_eq!(received["count"], 2);
    assert_eq!(received["stepId"], "step-1");
    assert_eq!(received["startLine"], 3);
    assert_eq!(received["value"], serde_json::json!(["a", "b"]));
    assert!(received.get("lines").is_none());
    Ok(())
}

struct Handshake {
    authorization: Option<String>,
    user_agent: Option<String>,
    query: Vec<(String, String)>,
}

struct Capture<'a>(&'a mut Option<Handshake>);

impl Callback for Capture<'_> {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        let header = |name: &str| {
            request
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned)
        };
        *self.0 = Some(Handshake {
            authorization: header("authorization"),
            user_agent: header("user-agent"),
            query: url::form_urlencoded::parse(
                request.uri().query().unwrap_or_default().as_bytes(),
            )
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect(),
        });
        Ok(response)
    }
}

async fn handshake_of(client: FeedStreamClient, listener: TcpListener) -> Result<Handshake> {
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut seen = None;
        let _socket = accept_hdr_async(stream, Capture(&mut seen)).await?;
        seen.context("handshake callback ran")
    });
    let _socket = client.connect().await?;
    server.await?
}

#[tokio::test]
async fn feed_stream_handshake_authenticates_and_routes_by_plan_and_job() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let client = FeedStreamClient::new(&format!("http://{address}/feed"), TOKEN)?;

    let plain = handshake_of(client.clone(), listener).await?;

    assert_eq!(plain.authorization, Some(format!("Bearer {TOKEN}")));
    assert_eq!(plain.user_agent.as_deref(), Some(RUNNER_USER_AGENT));
    assert_eq!(plain.query, Vec::<(String, String)>::new());

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let routed = FeedStreamClient::new(&format!("http://{address}/feed"), TOKEN)?
        .with_context("plan-1", "job-1");

    let routed = handshake_of(routed, listener).await?;

    assert_eq!(routed.authorization, Some(format!("Bearer {TOKEN}")));
    assert_eq!(
        routed.query,
        [
            ("planId".to_owned(), "plan-1".to_owned()),
            ("jobId".to_owned(), "job-1".to_owned())
        ]
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn feed_stream_connect_gives_up_after_thirty_seconds_when_the_handshake_stalls() -> Result<()>
{
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let held = tokio::spawn(async move {
        let accepted = listener.accept().await;
        std::future::pending::<()>().await;
        accepted
    });
    let client = FeedStreamClient::new(&format!("http://{address}/feed"), TOKEN)?;

    let started = tokio::time::Instant::now();
    let error = client
        .connect()
        .await
        .err()
        .context("a stalled handshake must fail")?;
    let elapsed = started.elapsed();

    assert!(
        error
            .downcast_ref::<tokio::time::error::Elapsed>()
            .is_some(),
        "{error:#}"
    );
    assert!(
        elapsed >= Duration::from_secs(30) && elapsed < Duration::from_secs(31),
        "{elapsed:?}"
    );
    held.abort();
    Ok(())
}
