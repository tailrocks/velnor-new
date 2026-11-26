// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Context, anyhow};

use super::LifecycleError;
use crate::protocol::{
    BrokerClient, BrokerErrorCategory, DecodedJitConfig, GitHubApiError, OAuthClient,
    OAuthJwtCredentials, RUNNER_JOB_REQUEST, RunnerJobRequestRef, RunnerStatus, TaskAgentMessage,
    TaskAgentSession, broker_error_category,
};
use crate::secret::Secret;

const SESSION_ATTEMPTS: u32 = 3;
const SESSION_RETRY_STEP: Duration = Duration::from_secs(2);
const MAX_CONSECUTIVE_POLL_ERRORS: u32 = 10;
const MAX_POLL_ERROR_DELAY: Duration = Duration::from_secs(60);
const HOSTED_RUNNER_SHUTDOWN: &str = "HostedRunnerShutdown";
const FORCE_TOKEN_REFRESH: &str = "ForceTokenRefresh";

#[derive(Debug, Clone)]
struct Auth {
    token: Secret<String>,
    broker: BrokerClient,
}

#[derive(Debug)]
pub(super) struct Session {
    oauth: OAuthClient,
    credentials: OAuthJwtCredentials,
    base_url: String,
    id: String,
    auth: Mutex<Auth>,
}

#[derive(Debug)]
pub(super) enum Assignment {
    Job(RunnerJobRequestRef),
    Shutdown,
}

fn credential<'a>(config: &'a DecodedJitConfig, name: &str) -> Option<&'a str> {
    config
        .credentials
        .data
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
        .filter(|value| !value.is_empty())
}

fn oauth_credentials(config: &DecodedJitConfig) -> anyhow::Result<OAuthJwtCredentials> {
    if !config.credentials.scheme.eq_ignore_ascii_case("OAuth") {
        return Err(anyhow!(
            "unsupported JIT credential scheme {}",
            config.credentials.scheme
        ));
    }
    let authorization_url = credential(config, "authorizationUrl")
        .context("JIT credentials have no authorizationUrl")?;
    Ok(OAuthJwtCredentials {
        client_id: credential(config, "clientId")
            .context("JIT credentials have no clientId")?
            .to_owned(),
        authorization_url: authorization_url.to_owned(),
        token_url: credential(config, "oauthEndpointUrl")
            .unwrap_or(authorization_url)
            .to_owned(),
        private_key_pem: config.private_key_pem.clone(),
    })
}

fn api_status(error: &anyhow::Error) -> Option<u16> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<GitHubApiError>())
        .map(|api| api.status)
}

fn is_auth_failure(error: &anyhow::Error) -> bool {
    matches!(api_status(error), Some(401 | 403))
}

fn is_retriable(error: &anyhow::Error) -> bool {
    api_status(error).is_none_or(|status| status >= 500 || matches!(status, 408 | 429))
}

pub(super) fn backoff(base: Duration, streak: u32) -> Duration {
    base.saturating_mul(
        1_u32
            .checked_shl(streak.saturating_sub(1))
            .unwrap_or(u32::MAX),
    )
    .min(MAX_POLL_ERROR_DELAY)
}

impl Session {
    pub(super) async fn open(config: &DecodedJitConfig) -> Result<Self, LifecycleError> {
        let credentials = oauth_credentials(config).map_err(LifecycleError::Config)?;
        let base_url = config
            .settings
            .server_url_v2
            .clone()
            .filter(|url| !url.is_empty())
            .ok_or_else(|| LifecycleError::Config(anyhow!("JIT settings have no ServerUrlV2")))?;
        let agent_id = config
            .settings
            .agent_id
            .ok_or_else(|| LifecycleError::Config(anyhow!("JIT settings have no AgentId")))?;
        let agent_name = config.settings.agent_name.clone().unwrap_or_default();
        let oauth = OAuthClient::new().map_err(LifecycleError::SignIn)?;
        let token = oauth
            .exchange_client_credentials(&credentials)
            .await
            .map_err(LifecycleError::SignIn)?
            .token;
        let broker =
            BrokerClient::new(&base_url, token.clone()).map_err(LifecycleError::Session)?;
        let owner = format!("{agent_name} (PID: {})", std::process::id());
        let created = create_session(&broker, TaskAgentSession::new(owner, agent_id, agent_name))
            .await
            .map_err(LifecycleError::Session)?;
        let id = created
            .session_id
            .filter(|id| !id.is_empty())
            .ok_or_else(|| LifecycleError::Session(anyhow!("broker session has no sessionId")))?;
        Ok(Self {
            oauth,
            credentials,
            base_url,
            id,
            auth: Mutex::new(Auth {
                token: Secret::new(token),
                broker,
            }),
        })
    }

    fn auth(&self) -> MutexGuard<'_, Auth> {
        self.auth
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn access_token(&self) -> String {
        self.auth().token.expose().clone()
    }

    async fn refresh(&self) -> anyhow::Result<()> {
        let token = self
            .oauth
            .exchange_client_credentials(&self.credentials)
            .await?
            .token;
        let broker = BrokerClient::new(&self.base_url, token.clone())?;
        *self.auth() = Auth {
            token: Secret::new(token),
            broker,
        };
        Ok(())
    }

    pub(super) async fn poll(
        &self,
        status: RunnerStatus,
    ) -> anyhow::Result<Option<TaskAgentMessage>> {
        let broker = self.auth().broker.clone();
        let polled = match broker.get_runner_message(&self.id, status, true).await {
            Err(error) if is_auth_failure(&error) => {
                self.refresh().await?;
                let broker = self.auth().broker.clone();
                broker.get_runner_message(&self.id, status, true).await
            }
            other => other,
        };
        polled.map(|poll| poll.message)
    }

    pub(super) async fn acknowledge(&self, runner_request_id: &str) -> anyhow::Result<()> {
        let broker = self.auth().broker.clone();
        broker
            .acknowledge_runner_request(&self.id, runner_request_id, RunnerStatus::Busy)
            .await
    }

    pub(super) async fn wait_for_job(
        &self,
        error_delay: Duration,
    ) -> Result<Assignment, LifecycleError> {
        let mut streak = 0_u32;
        loop {
            let message = match self.poll(RunnerStatus::Online).await {
                Ok(message) => message,
                Err(error) => {
                    streak += 1;
                    if !is_retriable(&error) || streak >= MAX_CONSECUTIVE_POLL_ERRORS {
                        return Err(LifecycleError::Poll(error));
                    }
                    eprintln!("warning: poll for a job failed, retrying: {error:#}");
                    tokio::time::sleep(backoff(error_delay, streak)).await;
                    continue;
                }
            };
            streak = 0;
            let Some(message) = message else {
                continue;
            };
            match message.message_type.as_str() {
                RUNNER_JOB_REQUEST => {
                    let request = serde_json::from_str(&message.body)
                        .context("parse RunnerJobRequest body")
                        .map_err(LifecycleError::Poll)?;
                    return Ok(Assignment::Job(request));
                }
                HOSTED_RUNNER_SHUTDOWN => return Ok(Assignment::Shutdown),
                FORCE_TOKEN_REFRESH => self.refresh().await.map_err(LifecycleError::Poll)?,
                _ => {}
            }
        }
    }

    pub(super) async fn close(&self) {
        let broker = self.auth().broker.clone();
        if let Err(error) = broker.delete_session(&self.id).await {
            eprintln!("warning: delete broker session: {error:#}");
        }
    }
}

async fn create_session(
    broker: &BrokerClient,
    session: TaskAgentSession,
) -> anyhow::Result<TaskAgentSession> {
    let mut attempt = 1_u32;
    loop {
        match broker.create_session(&session).await {
            Ok(created) => return Ok(created),
            Err(error)
                if attempt < SESSION_ATTEMPTS
                    && matches!(
                        broker_error_category(&error),
                        Some(BrokerErrorCategory::Transient | BrokerErrorCategory::Conflict)
                    ) =>
            {
                tokio::time::sleep(SESSION_RETRY_STEP * attempt).await;
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}
