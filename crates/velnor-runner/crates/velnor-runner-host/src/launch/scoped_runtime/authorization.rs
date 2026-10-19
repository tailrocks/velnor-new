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

/// Publish pre-start proofs and deliver JIT once.
pub(crate) async fn prepare_listener(
    ready: ChannelReadyBootstrap,
    docker: &bollard::Docker,
    host: &Journal,
    scoped: &ConnectionJournal,
    secret: &ReleaseReadSecret,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    deadline: Instant,
) -> Result<RunnerReadyBootstrap, PendingScopedExecution> {
    if let Err(error) = publish_job_facts_proof(
        &ready, docker, host, scoped, secret, engine, images, deadline,
    )
    .await
    {
        return Err(PendingScopedExecution::Channel(ready, error));
    }
    if let Err(error) = publish_challenge(
        &ready, docker, host, scoped, secret, engine, images, deadline,
    )
    .await
    {
        return Err(PendingScopedExecution::Channel(ready, error));
    }
    ready
        .deliver(scoped, deadline)
        .await
        .map_err(PendingScopedExecution::Jit)
}

async fn publish_job_facts_proof(
    ready: &ChannelReadyBootstrap,
    docker: &bollard::Docker,
    host: &Journal,
    scoped: &ConnectionJournal,
    secret: &ReleaseReadSecret,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    deadline: Instant,
) -> Result<(), HostError> {
    let reservation_id = ready.bound().prepared().acquired().reservation_id();
    let claim = match scoped.claim_job_facts_publication(reservation_id).await {
        Ok(Some(claim)) => claim,
        Ok(None) => return Err(HostError::Journal),
        Err(error) => return Err(error),
    };
    let proof = match crate::launch::resource_probe::publication::publish_job_facts(
        crate::launch::resource_probe::publication::JobFactsPublicationInput {
            docker,
            journal: host,
            connection_journal: scoped,
            selected: engine,
            images,
            release_secret: secret,
            ready: ready.bound().initialization(),
            claim: &claim,
            channel: ready.channel(),
            deadline,
        },
    )
    .await
    {
        Ok(proof) => proof,
        Err(error) => {
            return Err(error);
        }
    };
    if let Err(error) = scoped.record_job_facts_published(&claim, &proof).await {
        let _ = scoped.mark_job_facts_publication_uncertain(&claim).await;
        return Err(error);
    }
    Ok(())
}

async fn publish_challenge(
    ready: &ChannelReadyBootstrap,
    docker: &bollard::Docker,
    host: &Journal,
    scoped: &ConnectionJournal,
    secret: &ReleaseReadSecret,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    deadline: Instant,
) -> Result<(), HostError> {
    let reservation_id = ready.bound().prepared().acquired().reservation_id();
    let channel = ready.channel();
    let claim = match scoped
        .claim_execution_challenge(reservation_id, channel)
        .await
    {
        Ok(Some(claim)) => claim,
        Ok(None) => return Err(HostError::Journal),
        Err(error) => return Err(error),
    };
    let guest_boot_id = ready.bound().guest_boot_id();
    let receipt = match crate::launch::resource_probe::publication::publish_challenge(
        crate::launch::resource_probe::publication::ControlPublicationInput {
            docker,
            journal: host,
            selected: engine,
            images,
            release_secret: secret,
            metadata: ready.bound().metadata(),
            channel,
            guest_boot_id,
            deadline,
        },
        &claim,
    )
    .await
    {
        Ok(receipt) => receipt,
        Err(error) => {
            let _ = scoped.mark_execution_challenge_uncertain(&claim).await;
            return Err(error);
        }
    };
    if let Err(error) = scoped
        .record_execution_challenge_published(&claim, &receipt)
        .await
    {
        let _ = scoped.mark_execution_challenge_uncertain(&claim).await;
        return Err(error);
    }
    Ok(())
}

/// Verify one source after the session loop records its native start event.
pub(crate) async fn authorize_started_execution(
    runner: RunnerReadyBootstrap,
    docker: &bollard::Docker,
    host: &Journal,
    scoped: &ConnectionJournal,
    secret: &ReleaseReadSecret,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    connection: &RepositoryConnection,
    policy: &ApprovedJobAuthorityPolicy,
    github_token: &str,
    deadline: Instant,
) -> Result<AuthorizedScopedExecution, PendingScopedExecution> {
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
            let _ = scoped.mark_execution_decision_uncertain(&claim).await;
            return Err(error);
        }
    };
    if let Err(error) = scoped
        .record_execution_decision_delivered(&claim, &receipt)
        .await
    {
        let _ = scoped.mark_execution_decision_uncertain(&claim).await;
        return Err(error);
    }
    Ok(())
}
