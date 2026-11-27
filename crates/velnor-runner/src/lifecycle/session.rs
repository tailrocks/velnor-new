// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

mod create;

use std::future::Future;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Context, anyhow};

use super::LifecycleError;
use crate::context_value::ordinal_ignore_case_eq;
use crate::exec::JobCancel;
use crate::protocol::{
    Acknowledgement, BrokerClient, BrokerErrorCategory, DecodedJitConfig, GitHubApiError,
    OAuthClient, OAuthErrorKind, OAuthJwtCredentials, RUNNER_JOB_REQUEST, RunnerJobRequestRef,
    RunnerOs, RunnerStatus, SessionRecovery, TaskAgentMessage, TaskAgentSession, TokenBackoff,
    broker_error_category, oauth_error_kind,
};
use crate::secret::Secret;
use create::Token;

const SESSION_CONFLICT_ATTEMPTS: u32 = 8;
const CLOCK_SKEW_ATTEMPTS: u32 = 60;
const MAX_CONSECUTIVE_POLL_ERRORS: u32 = 10;
const MAX_POLL_ERROR_DELAY: Duration = Duration::from_secs(60);
const JOB_CANCELLATION: &str = "JobCancellation";
const HOSTED_RUNNER_SHUTDOWN: &str = "HostedRunnerShutdown";
const FORCE_TOKEN_REFRESH: &str = "ForceTokenRefresh";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MessageKind {
    JobRequest,
    JobCancellation,
    HostedRunnerShutdown,
    ForceTokenRefresh,
    Other,
}

impl MessageKind {
    pub(super) fn of(message: &TaskAgentMessage) -> Self {
        [
            (RUNNER_JOB_REQUEST, Self::JobRequest),
            (JOB_CANCELLATION, Self::JobCancellation),
            (HOSTED_RUNNER_SHUTDOWN, Self::HostedRunnerShutdown),
            (FORCE_TOKEN_REFRESH, Self::ForceTokenRefresh),
        ]
        .into_iter()
        .find(|(name, _)| ordinal_ignore_case_eq(&message.message_type, name))
        .map_or(Self::Other, |(_, kind)| kind)
    }
}

#[derive(Debug, Clone)]
struct Auth {
    token: Secret<String>,
    broker: BrokerClient,
    id: String,
}

#[derive(Debug)]
pub(super) struct Session {
    oauth: OAuthClient,
    credentials: OAuthJwtCredentials,
    base_url: String,
    os: RunnerOs,
    template: TaskAgentSession,
    retry_interval: Duration,
    recovery: SessionRecovery,
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
    api_status(error) == Some(401)
}

fn refused(mut error: anyhow::Error) -> anyhow::Error {
    if let Some(api) = error.downcast_mut::<GitHubApiError>()
        && api.status == 401
    {
        api.category.get_or_insert(BrokerErrorCategory::Terminal);
    }
    error
}

fn is_retriable(error: &anyhow::Error) -> bool {
    !matches!(
        oauth_error_kind(error),
        Some(OAuthErrorKind::Refused | OAuthErrorKind::RegistrationDeleted)
    ) && !matches!(
        broker_error_category(error),
        Some(BrokerErrorCategory::Terminal | BrokerErrorCategory::Expired)
    )
}

fn is_session_expired(error: &anyhow::Error) -> bool {
    broker_error_category(error) == Some(BrokerErrorCategory::Expired)
}

fn session_id(created: TaskAgentSession) -> anyhow::Result<String> {
    created
        .session_id
        .filter(|id| !id.is_empty())
        .ok_or_else(|| anyhow!("broker session has no sessionId"))
}

fn backoff(base: Duration, streak: u32) -> Duration {
    base.saturating_mul(
        1_u32
            .checked_shl(streak.saturating_sub(1))
            .unwrap_or(u32::MAX),
    )
    .min(MAX_POLL_ERROR_DELAY)
}

impl Session {
    pub(super) async fn open(
        config: &DecodedJitConfig,
        os: RunnerOs,
        retry_interval: Duration,
        token_backoff: TokenBackoff,
    ) -> Result<Self, LifecycleError> {
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
        let oauth = OAuthClient::new(token_backoff);
        let broker =
            BrokerClient::new(&base_url, os, String::new()).map_err(LifecycleError::Session)?;
        let owner = format!("{agent_name} (PID: {})", std::process::id());
        let template = TaskAgentSession::new(owner, agent_id, agent_name);
        let session = Self {
            oauth,
            credentials,
            base_url,
            os,
            template,
            retry_interval,
            recovery: config.settings.session_recovery,
            auth: Mutex::new(Auth {
                token: Secret::new(String::new()),
                broker,
                id: String::new(),
            }),
        };
        session.establish(Token::Missing).await.map_err(|error| {
            match oauth_error_kind(&error) {
                Some(_) => LifecycleError::SignIn(error),
                None => LifecycleError::Session(error),
            }
        })?;
        Ok(session)
    }

    fn auth(&self) -> MutexGuard<'_, Auth> {
        self.auth
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn access_token(&self) -> String {
        self.auth().token.expose().clone()
    }

    pub(super) async fn fresh_access_token(&self) -> anyhow::Result<String> {
        self.refresh().await?;
        Ok(self.access_token())
    }

    async fn refresh(&self) -> anyhow::Result<()> {
        let token = self
            .oauth
            .exchange_client_credentials(&self.credentials)
            .await?
            .token;
        let broker = BrokerClient::new(&self.base_url, self.os, token.clone())?;
        let mut auth = self.auth();
        auth.token = Secret::new(token);
        auth.broker = broker;
        Ok(())
    }

    async fn with_fresh_token<T, F, Fut>(&self, call: F) -> anyhow::Result<T>
    where
        F: Fn(BrokerClient) -> Fut,
        Fut: Future<Output = anyhow::Result<T>>,
    {
        let broker = self.auth().broker.clone();
        match call(broker).await {
            Err(error) if is_auth_failure(&error) => {
                self.refresh().await?;
                let broker = self.auth().broker.clone();
                call(broker).await.map_err(refused)
            }
            other => other,
        }
    }

    pub(super) async fn poll(
        &self,
        status: RunnerStatus,
    ) -> anyhow::Result<Option<TaskAgentMessage>> {
        loop {
            let id = self.auth().id.clone();
            let id = &id;
            let polled = self
                .with_fresh_token(|broker| async move {
                    broker.get_runner_message(id, status, true).await
                })
                .await;
            match polled {
                Err(error)
                    if self.recovery == SessionRecovery::Recreate && is_session_expired(&error) =>
                {
                    if let Err(failure) = self.establish(Token::Held).await {
                        return Err(error
                            .context(format!("recreate the expired broker session: {failure:#}")));
                    }
                    eprintln!("warning: the broker session expired, recreated it");
                }
                other => return self.settle(other?.message).await,
            }
        }
    }

    async fn settle(
        &self,
        message: Option<TaskAgentMessage>,
    ) -> anyhow::Result<Option<TaskAgentMessage>> {
        match message {
            Some(message) if MessageKind::of(&message) == MessageKind::ForceTokenRefresh => {
                self.refresh().await.map(|()| None)
            }
            message => Ok(message),
        }
    }

    pub(super) async fn acknowledge(
        &self,
        runner_request_id: &str,
    ) -> anyhow::Result<Acknowledgement> {
        let id = self.auth().id.clone();
        let id = &id;
        self.with_fresh_token(|broker| async move {
            broker
                .acknowledge_runner_request(id, runner_request_id, RunnerStatus::Busy)
                .await
        })
        .await
    }

    pub(super) async fn wait_for_job(
        &self,
        error_delay: Duration,
        shutdown: &JobCancel,
    ) -> Result<Assignment, LifecycleError> {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => Ok(Assignment::Shutdown),
            assignment = self.next_assignment(error_delay) => assignment,
        }
    }

    async fn next_assignment(&self, error_delay: Duration) -> Result<Assignment, LifecycleError> {
        let mut streak = 0_u32;
        loop {
            let message = match self.poll(RunnerStatus::Online).await {
                Ok(message) => message,
                Err(error) => {
                    streak += 1;
                    retry_poll(error, streak, error_delay).await?;
                    continue;
                }
            };
            streak = 0;
            let Some(message) = message else {
                continue;
            };
            match MessageKind::of(&message) {
                MessageKind::JobRequest => {
                    let request = serde_json::from_str(&message.body)
                        .context("parse RunnerJobRequest body")
                        .map_err(LifecycleError::Poll)?;
                    return Ok(Assignment::Job(request));
                }
                MessageKind::HostedRunnerShutdown => return Ok(Assignment::Shutdown),
                MessageKind::JobCancellation
                | MessageKind::ForceTokenRefresh
                | MessageKind::Other => {}
            }
        }
    }

    pub(super) async fn close(&self, timeout: Duration) {
        let id = self.auth().id.clone();
        let id = &id;
        let deleted = tokio::time::timeout(
            timeout,
            self.with_fresh_token(|broker| async move { broker.delete_session(id).await }),
        )
        .await;
        match deleted {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("warning: delete broker session: {error:#}"),
            Err(_) => eprintln!("warning: delete broker session: no answer within {timeout:?}"),
        }
    }
}

pub(super) async fn retry_poll(
    error: anyhow::Error,
    streak: u32,
    error_delay: Duration,
) -> Result<(), LifecycleError> {
    if !is_retriable(&error) || streak >= MAX_CONSECUTIVE_POLL_ERRORS {
        return Err(LifecycleError::Poll(error));
    }
    eprintln!("warning: polling the broker failed, retrying: {error:#}");
    tokio::time::sleep(backoff(error_delay, streak)).await;
    Ok(())
}
