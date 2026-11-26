// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use anyhow::{Context, Result, bail};
use futures_util::SinkExt;
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, USER_AGENT};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use url::Url;

use super::http::{RUNNER_USER_AGENT, validate_known_service_url};
use crate::secret::Secret;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

pub type FeedWebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

const FEED_HOSTS: &[&str] = &[
    "pipelines.actions.githubusercontent.com",
    "results-receiver.actions.githubusercontent.com",
];

#[derive(Serialize)]
struct FeedLines {
    count: usize,
    value: Vec<String>,
    #[serde(rename = "stepId")]
    step_id: String,
    #[serde(rename = "startLine")]
    start_line: i64,
}

#[derive(Debug, Clone)]
pub struct FeedStreamClient {
    url: Url,
    token: Secret<String>,
}

impl FeedStreamClient {
    pub fn new(feed_stream_url: &str, token: impl Into<String>) -> Result<Self> {
        Ok(Self {
            url: validate_feed_url(feed_stream_url)?,
            token: Secret::new(token.into()),
        })
    }

    #[must_use]
    pub fn from_endpoint_data(data: &BTreeMap<String, String>, token: &str) -> Option<Self> {
        let url = data.get("FeedStreamUrl").filter(|url| !url.is_empty())?;
        if token.is_empty() {
            return None;
        }
        Self::new(url, token).ok()
    }

    pub async fn connect(&self) -> Result<FeedWebSocket> {
        let mut request = self
            .url
            .as_str()
            .into_client_request()
            .context("build WebSocket request")?;
        let headers = request.headers_mut();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.token.expose()))
                .context("encode feed stream authorization")?,
        );
        headers.insert(USER_AGENT, HeaderValue::from_static(RUNNER_USER_AGENT));
        let (socket, _) = tokio::time::timeout(CONNECT_TIMEOUT, connect_async(request))
            .await
            .context("connect to feed stream WebSocket timed out")?
            .context("connect to feed stream WebSocket")?;
        Ok(socket)
    }

    pub async fn send_log_lines(
        socket: &mut FeedWebSocket,
        step_id: &str,
        lines: Vec<String>,
        start_line: i64,
    ) -> Result<()> {
        let feed = FeedLines {
            count: lines.len(),
            value: lines,
            step_id: step_id.to_owned(),
            start_line,
        };
        let json = serde_json::to_string(&feed).context("serialize feed lines")?;
        socket
            .send(Message::Text(json.into()))
            .await
            .context("send WebSocket log lines")
    }
}

fn validate_feed_url(raw: &str) -> Result<Url> {
    let mut url = Url::parse(raw).context("parse feed stream URL")?;
    let (http_scheme, ws_scheme) = match url.scheme() {
        "https" | "wss" => ("https", "wss"),
        "http" | "ws" => ("http", "ws"),
        other => bail!("feed stream URL must use https or wss, not {other}"),
    };
    let mut probe = url.clone();
    probe
        .set_scheme(http_scheme)
        .map_err(|()| anyhow::anyhow!("feed stream URL scheme cannot be validated"))?;
    validate_known_service_url(probe.as_str(), "FeedStreamUrl", FEED_HOSTS)?;
    url.set_scheme(ws_scheme)
        .map_err(|()| anyhow::anyhow!("feed stream URL scheme cannot be rewritten"))?;
    Ok(url)
}
