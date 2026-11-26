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

#[derive(Debug, Clone, Serialize)]
pub struct FeedLines {
    pub count: usize,
    pub value: Vec<String>,
    #[serde(rename = "stepId")]
    pub step_id: String,
    #[serde(rename = "startLine", skip_serializing_if = "Option::is_none")]
    pub start_line: Option<i64>,
    #[serde(rename = "planId", skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    #[serde(rename = "jobId", skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FeedStreamClient {
    url: Url,
    token: Secret<String>,
    route: Option<(String, String)>,
}

impl FeedStreamClient {
    pub fn new(feed_stream_url: &str, token: impl Into<String>) -> Result<Self> {
        Ok(Self {
            url: validate_feed_url(feed_stream_url)?,
            token: Secret::new(token.into()),
            route: None,
        })
    }

    #[must_use]
    pub fn with_context(mut self, plan_id: &str, job_id: &str) -> Self {
        self.route = Some((plan_id.to_owned(), job_id.to_owned()));
        self
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
        let mut url = self.url.clone();
        if let Some((plan_id, job_id)) = &self.route {
            url.query_pairs_mut()
                .append_pair("planId", plan_id)
                .append_pair("jobId", job_id);
        }
        let mut request = url
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
        start_line: Option<i64>,
        plan_id: Option<&str>,
        job_id: Option<&str>,
    ) -> Result<()> {
        let feed = FeedLines {
            count: lines.len(),
            value: lines,
            step_id: step_id.to_owned(),
            start_line,
            plan_id: plan_id.map(ToOwned::to_owned),
            job_id: job_id.map(ToOwned::to_owned),
        };
        let json = serde_json::to_string(&feed).context("serialize feed lines")?;
        socket
            .send(Message::Text(json.into()))
            .await
            .context("send WebSocket log lines")
    }

    pub async fn send_ping(socket: &mut FeedWebSocket) -> Result<()> {
        socket
            .send(Message::Ping(Vec::new().into()))
            .await
            .context("send WebSocket keepalive ping")
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
