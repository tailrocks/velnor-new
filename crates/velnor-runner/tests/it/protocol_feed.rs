// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use velnor_runner::protocol::FeedStreamClient;

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
        Some(3),
        None,
        None,
    )
    .await?;

    let received: Value = serde_json::from_str(&server.await??)?;
    assert_eq!(received["count"], 2);
    assert_eq!(received["stepId"], "step-1");
    assert_eq!(received["startLine"], 3);
    Ok(())
}
