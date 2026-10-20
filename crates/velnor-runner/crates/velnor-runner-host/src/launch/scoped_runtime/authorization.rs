//! Authorize one Runner job before its hook accepts the request.

use std::time::Instant;

use crate::{
    config::{ApprovedJobAuthorityPolicy, RepositoryConnection},
    error::HostError,
    journal::{
        ConnectionJournal, Journal,
        execution_authorization::{ExecutionProofProgress, StartedRequestTimeoutResult},
    },
    launch::{
        ReleaseReadSecret, SelectedEngineContext, VerifiedWorkerImages,
        execution_authorization::{AuthorizedExecutionSource, verify_execution_source},
    },
};

use super::jit::{ChannelReadyBootstrap, PendingJitBootstrap, RunnerReadyBootstrap};
use request_wait::{StartedRequestReadError, read_started_request};

/// Keep the exact live state when source proof cannot finish.
#[must_use]
pub(crate) enum PendingScopedExecution {
    Jit(PendingJitBootstrap),
    Channel(ChannelReadyBootstrap, HostError),
    RequestTimeout(RunnerReadyBootstrap),
    Runner(
        RunnerReadyBootstrap,
        Option<AuthorizedExecutionSource>,
        HostError,
    ),
}

impl PendingScopedExecution {
    pub(crate) fn intent_id(&self) -> i64 {
        let acquired = match self {
            Self::Jit(pending) => pending.bound().prepared().acquired(),
            Self::Channel(ready, _) => ready.bound().prepared().acquired(),
            Self::RequestTimeout(runner) => runner.bound().prepared().acquired(),
            Self::Runner(runner, _, _) => runner.bound().prepared().acquired(),
        };
        acquired.intent_id()
    }

    pub(crate) fn requires_cleanup_wake(&self) -> bool {
        matches!(self, Self::RequestTimeout(_))
    }
}

/// Keep the authorized source and live pair for terminal cleanup.
#[must_use]
pub(crate) struct AuthorizedScopedExecution {
    runner: RunnerReadyBootstrap,
    source: AuthorizedExecutionSource,
}

impl AuthorizedScopedExecution {
    pub(crate) fn runner(&self) -> &RunnerReadyBootstrap {
        &self.runner
    }

    pub(crate) fn source(&self) -> &AuthorizedExecutionSource {
        &self.source
    }

    pub(crate) fn intent_id(&self) -> i64 {
        self.source.request().intent_id()
    }
}

#[path = "authorization/context.rs"]
mod context;
pub(crate) use context::ScopedHostContext;

#[path = "authorization/publication.rs"]
mod publication;
pub(crate) use publication::prepare_listener;
#[cfg(test)]
pub(in crate::launch) use publication::prepare_listener_with_test_provider;

/// Verify one source after the session loop records its native start event.
pub(crate) async fn authorize_started_execution(
    runner: RunnerReadyBootstrap,
    context: &ScopedHostContext<'_>,
    connection: &RepositoryConnection,
    policy: &ApprovedJobAuthorityPolicy,
    github_token: &str,
    deadline: Instant,
) -> Result<AuthorizedScopedExecution, PendingScopedExecution> {
    let ScopedHostContext {
        docker,
        host,
        scoped,
        secret,
        engine,
        images,
    } = *context;
    let request =
        match read_started_request(scoped, &runner, docker, engine, images, deadline).await {
            Ok(request) => request,
            Err(StartedRequestReadError::DeadlineElapsed) => {
                let Some(request_id) = runner.request_id() else {
                    return Err(PendingScopedExecution::Runner(
                        runner,
                        None,
                        HostError::Identity,
                    ));
                };
                match scoped
                    .deny_started_request_timeout(runner.channel(), request_id)
                    .await
                {
                    Ok(
                        StartedRequestTimeoutResult::Enqueued
                        | StartedRequestTimeoutResult::AlreadyEnqueued,
                    ) => {
                        return Err(PendingScopedExecution::RequestTimeout(runner));
                    }
                    Ok(StartedRequestTimeoutResult::NotEligible) => {
                        return Err(PendingScopedExecution::Runner(
                            runner,
                            None,
                            HostError::Ownership,
                        ));
                    }
                    Err(error) => return Err(PendingScopedExecution::Runner(runner, None, error)),
                }
            }
            Err(StartedRequestReadError::Read(error)) => {
                return Err(PendingScopedExecution::Runner(runner, None, error));
            }
        };
    let source = match verify_execution_source(
        scoped,
        connection,
        policy,
        request.bytes(),
        runner.channel(),
        github_token,
        deadline,
    )
    .await
    {
        Ok(source) => source,
        Err(error) => return Err(PendingScopedExecution::Runner(runner, None, error)),
    };
    if let Err(error) = scoped.record_authorized_execution_source(&source).await {
        return Err(PendingScopedExecution::Runner(runner, Some(source), error));
    }
    if let Err(error) = prepare_decision(scoped, &source).await {
        return Err(PendingScopedExecution::Runner(runner, Some(source), error));
    }
    if let Err(error) = publish_decision(
        &runner, &source, docker, host, scoped, secret, engine, images, deadline,
    )
    .await
    {
        return Err(PendingScopedExecution::Runner(runner, Some(source), error));
    }
    Ok(AuthorizedScopedExecution { runner, source })
}

#[path = "authorization/request_wait.rs"]
mod request_wait;

async fn prepare_decision(
    scoped: &ConnectionJournal,
    source: &AuthorizedExecutionSource,
) -> Result<(), HostError> {
    match scoped.prepare_execution_decision(source).await? {
        ExecutionProofProgress::DecisionPrepared => Ok(()),
        _ => Err(HostError::Journal),
    }
}

async fn publish_decision(
    runner: &RunnerReadyBootstrap,
    source: &AuthorizedExecutionSource,
    docker: &bollard::Docker,
    host: &Journal,
    scoped: &ConnectionJournal,
    secret: &ReleaseReadSecret,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    deadline: Instant,
) -> Result<(), HostError> {
    let claim = scoped
        .claim_execution_decision_delivery(source.request().intent_id())
        .await?
        .ok_or(HostError::Journal)?;
    let receipt = match crate::launch::resource_probe::publication::publish_decision(
        crate::launch::resource_probe::publication::ControlPublicationInput {
            docker,
            journal: host,
            selected: engine,
            images,
            release_secret: secret,
            metadata: runner.bound().metadata(),
            channel: runner.channel(),
            guest_boot_id: runner.bound().guest_boot_id(),
            deadline,
        },
        &claim,
    )
    .await
    {
        Ok(receipt) => receipt,
        Err(error) => {
            scoped.mark_execution_decision_uncertain(&claim).await?;
            return Err(error);
        }
    };
    if let Err(error) = scoped
        .record_execution_decision_delivered(&claim, &receipt)
        .await
    {
        scoped.mark_execution_decision_uncertain(&claim).await?;
        return Err(error);
    }
    Ok(())
}
