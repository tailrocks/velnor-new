// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use anyhow::{Context, Result};
use reqwest::Method;
use serde_json::json;
use std::time::Duration;
use url::Url;

use super::error::{
    BrokerErrorCategory, ServiceErrorKind, broker_error_kind, github_api_error,
    github_api_error_categorized, github_api_error_with, is_retriable_status,
};
use super::http::{
    github_json_request, runner_architecture, runner_os, slash_url, validate_authenticated_url,
};
use super::session::{RunnerStatus, TaskAgentMessage, TaskAgentSession};
use crate::secret::Secret;
use velnor_model::RUNNER_PROTOCOL_VERSION;

const SESSION_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_TIMEOUT: Duration = Duration::from_secs(70);
const ACKNOWLEDGE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct BrokerPoll {
    pub status: u16,
    pub message: Option<TaskAgentMessage>,
}

#[derive(Debug, Clone)]
pub struct BrokerClient {
    base_url: Url,
    bearer_token: Secret<String>,
}

impl BrokerClient {
    pub fn new(server_url_v2: &str, bearer_token: impl Into<String>) -> Result<Self> {
        let url = validate_authenticated_url(server_url_v2)?;
        Ok(Self {
            base_url: slash_url(&url),
            bearer_token: Secret::new(bearer_token.into()),
        })
    }

    #[must_use]
    pub fn base_url_str(&self) -> String {
        self.base_url.to_string()
    }

    pub async fn create_session(&self, session: &TaskAgentSession) -> Result<TaskAgentSession> {
        let url = self.endpoint("session")?;
        let body = serde_json::to_string(session).context("serialize session")?;
        let (status, text) = self
            .send(Method::POST, &url, Some(body), SESSION_TIMEOUT)
            .await?;
        if !(200..300).contains(&status) {
            return Err(github_api_error_categorized(
                "create broker session",
                status,
                &text,
                session_error_category(status),
            ));
        }
        serde_json::from_str(&text).context("parse create broker session response")
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let mut url = self.endpoint("session")?;
        url.query_pairs_mut().append_pair("sessionId", session_id);
        let (status, text) = self
            .send(Method::DELETE, &url, None, SESSION_TIMEOUT)
            .await?;
        if !(200..300).contains(&status) {
            return Err(github_api_error("delete broker session", status, &text));
        }
        Ok(())
    }

    pub async fn get_runner_message(
        &self,
        session_id: &str,
        status: RunnerStatus,
        disable_update: bool,
    ) -> Result<BrokerPoll> {
        let mut url = self.endpoint("message")?;
        append_runner_query(&mut url, session_id, status)?;
        url.query_pairs_mut().append_pair(
            "disableUpdate",
            if disable_update { "true" } else { "false" },
        );
        let (http_status, text) = self.send(Method::GET, &url, None, POLL_TIMEOUT).await?;
        if !(200..300).contains(&http_status) {
            return Err(broker_api_error(
                "get broker message",
                http_status,
                text.trim(),
                None,
                POLL_KINDS,
            ));
        }
        if http_status == 204 || text.trim().is_empty() {
            return Ok(BrokerPoll {
                status: http_status,
                message: None,
            });
        }
        let message =
            serde_json::from_str(text.trim()).context("parse get broker message response")?;
        Ok(BrokerPoll {
            status: http_status,
            message: Some(message),
        })
    }

    pub async fn acknowledge_runner_request(
        &self,
        session_id: &str,
        runner_request_id: &str,
        status: RunnerStatus,
    ) -> Result<()> {
        let mut url = self.endpoint("acknowledge")?;
        append_runner_query(&mut url, session_id, status)?;
        let body = json!({ "runnerRequestId": runner_request_id }).to_string();
        let (http_status, text) = self
            .send(Method::POST, &url, Some(body), ACKNOWLEDGE_TIMEOUT)
            .await?;
        if !(200..300).contains(&http_status) {
            return Err(broker_api_error(
                "acknowledge broker runner request",
                http_status,
                &text,
                Some(session_error_category(http_status)),
                ACKNOWLEDGE_KINDS,
            ));
        }
        Ok(())
    }

    fn endpoint(&self, name: &str) -> Result<Url> {
        self.base_url
            .join(name)
            .with_context(|| format!("build broker {name} URL"))
    }

    async fn send(
        &self,
        method: Method,
        url: &Url,
        body: Option<String>,
        timeout: Duration,
    ) -> Result<(u16, String)> {
        github_json_request(
            method,
            url.as_str(),
            self.bearer_token.expose(),
            body,
            timeout,
        )
        .await
    }
}

const POLL_KINDS: &[ServiceErrorKind] = &[
    ServiceErrorKind::RunnerNotFound,
    ServiceErrorKind::RunnerVersionTooOld,
    ServiceErrorKind::HostedRunnerDeprovisioned,
    ServiceErrorKind::RunnerSessionInvalid,
];

const ACKNOWLEDGE_KINDS: &[ServiceErrorKind] = &[
    ServiceErrorKind::AcknowledgeJobNotFound,
    ServiceErrorKind::RunnerNotFound,
];

fn broker_api_error(
    action: &str,
    status: u16,
    body: &str,
    category: Option<BrokerErrorCategory>,
    kinds: &[ServiceErrorKind],
) -> anyhow::Error {
    github_api_error_with(
        action,
        status,
        body,
        category,
        broker_error_kind(body, kinds),
    )
}

fn session_error_category(status: u16) -> BrokerErrorCategory {
    match status {
        409 => BrokerErrorCategory::Conflict,
        status if is_retriable_status(status) => BrokerErrorCategory::Transient,
        _ => BrokerErrorCategory::Terminal,
    }
}

fn append_runner_query(url: &mut Url, session_id: &str, status: RunnerStatus) -> Result<()> {
    url.query_pairs_mut()
        .append_pair("sessionId", session_id)
        .append_pair("status", status.as_query_value())
        .append_pair("runnerVersion", RUNNER_PROTOCOL_VERSION)
        .append_pair("os", runner_os()?)
        .append_pair("architecture", runner_architecture()?);
    Ok(())
}
