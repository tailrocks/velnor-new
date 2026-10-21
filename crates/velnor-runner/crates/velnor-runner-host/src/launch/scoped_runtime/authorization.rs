//! Authorize one Runner job before its hook accepts the request.

use std::time::Instant;

use crate::{
    config::{ApprovedJobAuthorityPolicy, RepositoryConnection},
    error::HostError,
    journal::{
        ConnectionJournal,
        execution_authorization::{ExecutionProofProgress, StartedRequestTimeoutResult},
    },
    launch::{
        SelectedEngineContext, VerifiedWorkerImages,
        execution_authorization::{AuthorizedExecutionSource, verify_execution_source},
    },
    worker::execution_authorization::VerifiedExecutionRequest,
};

use super::jit::{ChannelReadyBootstrap, PendingJitBootstrap, RunnerReadyBootstrap};
use request_wait::{StartedRequestReadError, read_started_request};

/// Keep the exact live state when source proof cannot finish.
#[must_use]
pub(crate) enum PendingScopedExecution {
    Jit(PendingJitBootstrap),
    Channel(Box<ChannelReadyBootstrap>, HostError),
    RequestTimeout(Box<RunnerReadyBootstrap>),
    Runner(
        Box<RunnerReadyBootstrap>,
        Option<Box<AuthorizedExecutionSource>>,
        HostError,
    ),
}

impl PendingScopedExecution {
    pub(crate) fn intent_id(&self) -> i64 {
        let acquired = match self {
            Self::Jit(pending) => pending.bound().prepared().acquired(),
            Self::Channel(ready, _) => ready.bound().prepared().acquired(),
            Self::RequestTimeout(runner) | Self::Runner(runner, _, _) => {
                runner.bound().prepared().acquired()
            }
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
    // Keep the exact delivered runner alive until terminal cleanup completes.
    _runner: RunnerReadyBootstrap,
    source: AuthorizedExecutionSource,
}

#[cfg(test)]
#[test]
fn pending_scoped_execution_error_stays_bounded() {
    assert!(std::mem::size_of::<PendingScopedExecution>() <= 128);
}

impl AuthorizedScopedExecution {
    pub(crate) fn source(&self) -> &AuthorizedExecutionSource {
        &self.source
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
    let (runner, request) = Box::pin(read_request_or_pending(
        runner, scoped, docker, engine, images, deadline,
    ))
    .await?;
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
        Err(error) => {
            return Err(PendingScopedExecution::Runner(
                Box::new(runner),
                None,
                error,
            ));
        }
    };
    if let Err(error) = scoped.record_authorized_execution_source(&source).await {
        return Err(PendingScopedExecution::Runner(
            Box::new(runner),
            Some(Box::new(source)),
            error,
        ));
    }
    if let Err(error) = prepare_decision(scoped, &source).await {
        return Err(PendingScopedExecution::Runner(
            Box::new(runner),
            Some(Box::new(source)),
            error,
        ));
    }
    if let Err(error) = publish_decision(
        &runner,
        &source,
        ScopedHostContext {
            docker,
            host,
            scoped,
            secret,
            engine,
            images,
        },
        deadline,
    )
    .await
    {
        return Err(PendingScopedExecution::Runner(
            Box::new(runner),
            Some(Box::new(source)),
            error,
        ));
    }
    Ok(AuthorizedScopedExecution {
        _runner: runner,
        source,
    })
}

async fn read_request_or_pending(
    runner: RunnerReadyBootstrap,
    scoped: &ConnectionJournal,
    docker: &bollard::Docker,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    deadline: Instant,
) -> Result<(RunnerReadyBootstrap, VerifiedExecutionRequest), PendingScopedExecution> {
    let request =
        match read_started_request(scoped, &runner, docker, engine, images, deadline).await {
            Ok(request) => request,
            Err(StartedRequestReadError::DeadlineElapsed) => {
                let Some(request_id) = runner.request_id() else {
                    return Err(PendingScopedExecution::Runner(
                        Box::new(runner),
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
                    ) => return Err(PendingScopedExecution::RequestTimeout(Box::new(runner))),
                    Ok(StartedRequestTimeoutResult::NotEligible) => {
                        return Err(PendingScopedExecution::Runner(
                            Box::new(runner),
                            None,
                            HostError::Ownership,
                        ));
                    }
                    Err(error) => {
                        return Err(PendingScopedExecution::Runner(
                            Box::new(runner),
                            None,
                            error,
                        ));
                    }
                }
            }
            Err(StartedRequestReadError::Read(error)) => {
                return Err(PendingScopedExecution::Runner(
                    Box::new(runner),
                    None,
                    error,
                ));
            }
        };
    Ok((runner, request))
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
    context: ScopedHostContext<'_>,
    deadline: Instant,
) -> Result<(), HostError> {
    let claim = context
        .scoped
        .claim_execution_decision_delivery(source.request().intent_id())
        .await?
        .ok_or(HostError::Journal)?;
    let receipt = match crate::launch::resource_probe::publication::publish_decision(
        crate::launch::resource_probe::publication::ControlPublicationInput {
            docker: context.docker,
            journal: context.host,
            selected: context.engine,
            images: context.images,
            release_secret: context.secret,
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
            context
                .scoped
                .mark_execution_decision_uncertain(&claim)
                .await?;
            return Err(error);
        }
    };
    if let Err(error) = context
        .scoped
        .record_execution_decision_delivered(&claim, &receipt)
        .await
    {
        context
            .scoped
            .mark_execution_decision_uncertain(&claim)
            .await?;
        return Err(error);
    }
    Ok(())
}
