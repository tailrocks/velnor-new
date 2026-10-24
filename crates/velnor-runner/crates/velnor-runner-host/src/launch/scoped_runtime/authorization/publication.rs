use std::{future::Future, time::Instant};

use crate::{
    config::RepositoryConnection,
    error::HostError,
    launch::scoped_runtime::{ChannelReadyBootstrap, ScopedHostContext},
};

use super::PendingScopedExecution;
use crate::launch::scoped_runtime::RunnerReadyBootstrap;

/// Publish pre-start proofs and deliver JIT once.
pub(crate) async fn prepare_listener(
    ready: ChannelReadyBootstrap,
    context: &ScopedHostContext<'_>,
    connection: &RepositoryConnection,
    guest_image: &crate::worker::guest_image::GuestImageFile,
    deadline: Instant,
) -> Result<RunnerReadyBootstrap, PendingScopedExecution> {
    let provider =
        crate::launch::resource_probe::release_image_provider(context.secret, context.host);
    let provider = provider.as_ref().map_err(|error| *error);
    Box::pin(prepare_listener_with_provider(
        ready,
        context,
        connection,
        guest_image,
        provider,
        |_| async { Ok(()) },
        deadline,
    ))
    .await
}

#[cfg(test)]
pub(in crate::launch) async fn prepare_listener_with_test_provider<P, F, Fut>(
    ready: ChannelReadyBootstrap,
    context: &ScopedHostContext<'_>,
    connection: &RepositoryConnection,
    guest_image: &crate::worker::guest_image::GuestImageFile,
    provider: &P,
    before_delivery: F,
    deadline: Instant,
) -> Result<RunnerReadyBootstrap, PendingScopedExecution>
where
    P: crate::launch::resource_probe::ImageProvider,
    F: FnOnce(i64) -> Fut,
    Fut: Future<Output = Result<(), HostError>>,
{
    prepare_listener_with_provider(
        ready,
        context,
        connection,
        guest_image,
        Ok(provider),
        before_delivery,
        deadline,
    )
    .await
}

async fn prepare_listener_with_provider<P, F, Fut>(
    ready: ChannelReadyBootstrap,
    context: &ScopedHostContext<'_>,
    connection: &RepositoryConnection,
    guest_image: &crate::worker::guest_image::GuestImageFile,
    provider: Result<&P, HostError>,
    before_delivery: F,
    deadline: Instant,
) -> Result<RunnerReadyBootstrap, PendingScopedExecution>
where
    P: crate::launch::resource_probe::ImageProvider,
    F: FnOnce(i64) -> Fut,
    Fut: Future<Output = Result<(), HostError>>,
{
    if let Err(error) = publish_job_facts_proof_with_provider(
        &ready,
        context,
        connection,
        guest_image,
        provider,
        deadline,
    )
    .await
    {
        return Err(PendingScopedExecution::Channel(Box::new(ready), error));
    }
    if let Err(error) = publish_challenge_with_provider(&ready, context, provider, deadline).await {
        return Err(PendingScopedExecution::Channel(Box::new(ready), error));
    }
    let reservation_id = ready.bound().prepared().acquired().reservation_id();
    if let Err(error) = before_delivery(reservation_id).await {
        return Err(PendingScopedExecution::Channel(Box::new(ready), error));
    }
    ready
        .deliver(context.scoped, deadline)
        .await
        .map_err(PendingScopedExecution::Jit)
}

async fn publish_job_facts_proof_with_provider<P>(
    ready: &ChannelReadyBootstrap,
    context: &ScopedHostContext<'_>,
    connection: &RepositoryConnection,
    guest_image: &crate::worker::guest_image::GuestImageFile,
    provider: Result<&P, HostError>,
    deadline: Instant,
) -> Result<(), HostError>
where
    P: crate::launch::resource_probe::ImageProvider,
{
    let reservation_id = ready.bound().prepared().acquired().reservation_id();
    let claim = context
        .scoped
        .claim_job_facts_publication(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let provider = match provider {
        Ok(provider) => provider,
        Err(error) => {
            context
                .scoped
                .mark_job_facts_publication_uncertain(&claim)
                .await?;
            return Err(error);
        }
    };
    let input = crate::launch::resource_probe::publication::JobFactsPublicationInput {
        docker: context.docker,
        journal: context.host,
        connection_journal: context.scoped,
        selected: context.engine,
        images: context.images,
        connection,
        guest_image,
        ready: ready.bound().initialization(),
        claim: &claim,
        channel: ready.channel(),
        deadline,
    };
    let proof = crate::launch::resource_probe::publication::publish_job_facts_with_provider(
        input, provider,
    )
    .await;
    let proof = match proof {
        Ok(proof) => proof,
        Err(error) => return Err(error),
    };
    if let Err(error) = context
        .scoped
        .record_job_facts_published(&claim, &proof)
        .await
    {
        context
            .scoped
            .mark_job_facts_publication_uncertain(&claim)
            .await?;
        return Err(error);
    }
    Ok(())
}

async fn publish_challenge_with_provider<P>(
    ready: &ChannelReadyBootstrap,
    context: &ScopedHostContext<'_>,
    provider: Result<&P, HostError>,
    deadline: Instant,
) -> Result<(), HostError>
where
    P: crate::launch::resource_probe::ImageProvider,
{
    let reservation_id = ready.bound().prepared().acquired().reservation_id();
    let channel = ready.channel();
    let claim = context
        .scoped
        .claim_execution_challenge(reservation_id, channel)
        .await?
        .ok_or(HostError::Journal)?;
    let provider = match provider {
        Ok(provider) => provider,
        Err(error) => {
            context
                .scoped
                .mark_execution_challenge_uncertain(&claim)
                .await?;
            return Err(error);
        }
    };
    let receipt = crate::launch::resource_probe::publication::publish_challenge_with_provider(
        crate::launch::resource_probe::publication::ControlPublicationInput {
            docker: context.docker,
            journal: context.host,
            selected: context.engine,
            images: context.images,
            release_secret: context.secret,
            metadata: ready.bound().metadata(),
            channel,
            guest_boot_id: ready.bound().guest_boot_id(),
            deadline,
        },
        &claim,
        provider,
    )
    .await;
    let receipt = match receipt {
        Ok(receipt) => receipt,
        Err(error) => {
            context
                .scoped
                .mark_execution_challenge_uncertain(&claim)
                .await?;
            return Err(error);
        }
    };
    if let Err(error) = context
        .scoped
        .record_execution_challenge_published(&claim, &receipt)
        .await
    {
        context
            .scoped
            .mark_execution_challenge_uncertain(&claim)
            .await?;
        return Err(error);
    }
    Ok(())
}
