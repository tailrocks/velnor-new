//! Prepare one acquired job through measured storage binding.

mod authorization;
mod binding;
mod channel_outcome;
mod jit;
mod provider_entry;
mod registry;
#[cfg(test)]
mod test_provider;

#[cfg(test)]
#[path = "scoped_runtime/future_size_tests.rs"]
mod future_size_tests;
#[cfg(test)]
#[path = "scoped_runtime/stale_acquired_tests.rs"]
mod stale_acquired_tests;

#[cfg(test)]
pub(in crate::launch) use authorization::prepare_listener_with_test_provider;
pub(crate) use authorization::{
    AuthorizedScopedExecution, PendingScopedExecution, ScopedHostContext,
    authorize_started_execution, prepare_listener,
};
use binding::{bind_started, cleanup_failed_pair};
pub(crate) use jit::{
    ChannelReadyBootstrap, PendingChannelBootstrap, PendingJitBootstrap, RunnerReadyBootstrap,
    create_and_bind,
};
pub(crate) use provider_entry::prepare_and_bind_scoped;
#[cfg(test)]
pub(in crate::launch) use provider_entry::prepare_and_bind_scoped_with_test_provider;
pub(crate) use registry::RunnerReadyRegistry;
#[cfg(test)]
pub(crate) use test_provider::prepare_jit_and_measure_scoped_with_provider;

use bollard::Docker;
use std::{future::Future, pin::Pin, time::Instant};
use velnor_runner_github::Transport;

use crate::{
    error::HostError,
    journal::{ConnectionJournal, Journal, RecordedStorageNetwork, VerifiedMetadataInitialization},
    launch::{
        Lane, SelectedEngineContext, VerifiedWorkerImages,
        bootstrap::{PendingBootstrapPair, PreparedAcquiredBootstrap, StartedBootstrapPair},
        resource_probe::{ImageProvider, ReleaseReadSecret},
        steps::ScopedProvisioningOutcome,
    },
    worker::storage_initialization::VerifiedStorageInitializationReady,
};

/// Keep the acquired job and its started pair after storage binding.
#[must_use]
pub(crate) enum ScopedRuntimeOutcome {
    NoStart,
    Bound(BoundScopedBootstrap),
    Pending(PendingScopedBootstrap),
}

/// Keep each exact outcome from preparation through channel measurement.
#[must_use]
pub(crate) enum ScopedChannelOutcome {
    NoStart,
    Ready(ChannelReadyBootstrap),
    PendingPreparation(PendingScopedBootstrap),
    PendingJit(PendingJitBootstrap),
    PendingChannel(PendingChannelBootstrap),
}

pub(crate) type ScopedPrepareFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ScopedRuntimeOutcome, HostError>> + 'a>>;

/// Keep the prepared state and the durable network proof after binding.
#[must_use]
pub(crate) struct BoundScopedBootstrap {
    prepared: PreparedAcquiredBootstrap,
    network: RecordedStorageNetwork,
    started: StartedBootstrapPair,
    metadata: VerifiedMetadataInitialization,
    initialization: VerifiedStorageInitializationReady,
    binding: super::bootstrap::VerifiedStorageResourceBinding,
    guest_boot_id: String,
}

impl BoundScopedBootstrap {
    pub(crate) fn prepared(&self) -> &PreparedAcquiredBootstrap {
        &self.prepared
    }

    pub(crate) fn network(&self) -> &RecordedStorageNetwork {
        &self.network
    }

    pub(crate) fn started(&self) -> &StartedBootstrapPair {
        &self.started
    }

    pub(crate) fn metadata(&self) -> &VerifiedMetadataInitialization {
        &self.metadata
    }

    pub(crate) fn initialization(&self) -> &VerifiedStorageInitializationReady {
        &self.initialization
    }

    pub(crate) fn binding(&self) -> &super::bootstrap::VerifiedStorageResourceBinding {
        &self.binding
    }

    pub(crate) fn guest_boot_id(&self) -> &str {
        &self.guest_boot_id
    }
}

/// Keep live handles when a later step fails.
#[must_use]
pub(crate) struct PendingScopedBootstrap {
    prepared: PreparedAcquiredBootstrap,
    network: Option<RecordedStorageNetwork>,
    started: Option<StartedBootstrapPair>,
    pending_pair: Option<PendingBootstrapPair>,
    initialization: Option<VerifiedStorageInitializationReady>,
    failure: HostError,
}

impl PendingScopedBootstrap {
    pub(crate) fn prepared(&self) -> &PreparedAcquiredBootstrap {
        &self.prepared
    }

    pub(crate) fn network(&self) -> Option<&RecordedStorageNetwork> {
        self.network.as_ref()
    }

    pub(crate) fn started(&self) -> Option<&StartedBootstrapPair> {
        self.started.as_ref()
    }

    pub(crate) fn pending_pair(&self) -> Option<&PendingBootstrapPair> {
        self.pending_pair.as_ref()
    }

    pub(crate) fn initialization(&self) -> Option<&VerifiedStorageInitializationReady> {
        self.initialization.as_ref()
    }

    pub(crate) fn failure(&self) -> &HostError {
        &self.failure
    }
}

struct ScopedPrepareContext<'a, P> {
    docker: &'a Docker,
    scoped: &'a ConnectionJournal,
    host: &'a Journal,
    provider: &'a P,
    engine: &'a SelectedEngineContext,
    images: &'a VerifiedWorkerImages,
}

fn prepare_and_bind_scoped_with_provider<'a, P: ImageProvider + 'a>(
    outcome: ScopedProvisioningOutcome,
    context: ScopedPrepareContext<'a, P>,
) -> ScopedPrepareFuture<'a> {
    Box::pin(async move {
        let prepared = match super::bootstrap::adapter::prepare_acquired(
            outcome,
            context.docker,
            context.scoped,
            context.engine,
            context.images,
        )
        .await
        {
            Ok(prepared) => prepared,
            Err(error) => return Err(error),
        };
        let Some(prepared) = prepared else {
            return Ok(ScopedRuntimeOutcome::NoStart);
        };
        Box::pin(prepare_started_and_bound(
            prepared,
            context.docker,
            context.scoped,
            context.host,
            context.provider,
            context.engine,
            context.images,
        ))
        .await
    })
}

fn scoped_prepare_context<'a, P: ImageProvider + 'a>(
    docker: &'a Docker,
    scoped: &'a ConnectionJournal,
    host: &'a Journal,
    provider: &'a P,
    engine: &'a SelectedEngineContext,
    images: &'a VerifiedWorkerImages,
) -> ScopedPrepareContext<'a, P> {
    ScopedPrepareContext {
        docker,
        scoped,
        host,
        provider,
        engine,
        images,
    }
}

/// Prepare one job, bind its exact Runner, and measure its proof channel.
pub(crate) async fn prepare_jit_and_measure_scoped<T>(
    outcome: ScopedProvisioningOutcome,
    docker: &Docker,
    scoped: &ConnectionJournal,
    host: &Journal,
    release_secret: &ReleaseReadSecret,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    lane: &mut T,
    admin_token: &str,
    deadline: Instant,
) -> Result<ScopedChannelOutcome, HostError>
where
    T: Transport + Lane,
{
    let bound = match prepare_and_bind_scoped(
        outcome,
        docker,
        scoped,
        host,
        release_secret,
        engine,
        images,
    )
    .await?
    {
        ScopedRuntimeOutcome::NoStart => return Ok(ScopedChannelOutcome::NoStart),
        ScopedRuntimeOutcome::Pending(pending) => {
            return Ok(ScopedChannelOutcome::PendingPreparation(pending));
        }
        ScopedRuntimeOutcome::Bound(bound) => bound,
    };
    let jit = match create_and_bind(bound, lane, admin_token, scoped).await {
        Ok(jit) => jit,
        Err(pending) => return Ok(ScopedChannelOutcome::PendingJit(pending)),
    };
    match jit.measure_channel(docker, engine, images, deadline).await {
        Ok(ready) => Ok(ScopedChannelOutcome::Ready(ready)),
        Err(pending) => Ok(ScopedChannelOutcome::PendingChannel(pending)),
    }
}

async fn prepare_started_and_bound<P: ImageProvider>(
    prepared: PreparedAcquiredBootstrap,
    docker: &Docker,
    scoped: &ConnectionJournal,
    host: &Journal,
    provider: &P,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
) -> Result<ScopedRuntimeOutcome, HostError> {
    let StartedPairPreparation {
        mut prepared,
        network,
        started,
    } = match Box::pin(prepare_network_and_started_pair(
        prepared, docker, scoped, host, provider, engine, images,
    ))
    .await
    {
        Ok(preparation) => preparation,
        Err(pending) => return Ok(pending),
    };
    let reservation_id = prepared.acquired().reservation_id();
    let initialization = match Box::pin(
        crate::worker::storage_initialization::finish_after_pair_mounts(
            docker,
            scoped,
            prepared.storage_mut().pending_initialization_mut(),
            started.mount_handoff(),
        ),
    )
    .await
    {
        Ok(initialization) => initialization,
        Err(error) => return Ok(pending(prepared, Some(network), Some(started), None, error)),
    };
    let metadata = match scoped.metadata_initialization_ready(reservation_id).await {
        Ok(Some(metadata)) => metadata,
        Ok(None) => {
            return Ok(pending_with_initialization(
                prepared,
                Some(network),
                Some(started),
                None,
                Some(initialization),
                HostError::Journal,
            ));
        }
        Err(error) => {
            return Ok(pending_with_initialization(
                prepared,
                Some(network),
                Some(started),
                None,
                Some(initialization),
                error,
            ));
        }
    };
    Box::pin(bind_started(
        prepared,
        network,
        started,
        metadata,
        initialization,
        docker,
        scoped,
        host,
        provider,
        engine,
        images,
    ))
    .await
}

struct StartedPairPreparation {
    prepared: PreparedAcquiredBootstrap,
    network: RecordedStorageNetwork,
    started: StartedBootstrapPair,
}

async fn prepare_network_and_started_pair<P: ImageProvider>(
    prepared: PreparedAcquiredBootstrap,
    docker: &Docker,
    scoped: &ConnectionJournal,
    host: &Journal,
    provider: &P,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
) -> Result<StartedPairPreparation, ScopedRuntimeOutcome> {
    let reservation_id = prepared.acquired().reservation_id();
    let Some(initializer) = prepared.storage().pending_initialization() else {
        return Err(pending(prepared, None, None, None, HostError::Journal));
    };
    let network = match Box::pin(super::bootstrap::create_owned_network(
        docker,
        scoped,
        engine,
        reservation_id,
    ))
    .await
    {
        Ok(network) => network,
        Err(error) => return Err(pending(prepared, None, None, None, error)),
    };
    let started = match Box::pin(super::bootstrap::start_bootstrap_pair(
        &prepared,
        &network,
        initializer.metadata_handoff(),
        initializer,
        docker,
        scoped,
        engine,
        images,
    ))
    .await
    {
        Ok(started) => started,
        Err(failure) => {
            let (error, pending_pair) = failure.into_parts();
            return Err(cleanup_failed_pair(
                prepared,
                network,
                error,
                pending_pair,
                docker,
                scoped,
                host,
                provider,
                engine,
                images,
                reservation_id,
            )
            .await);
        }
    };
    Ok(StartedPairPreparation {
        prepared,
        network,
        started,
    })
}

fn pending(
    prepared: PreparedAcquiredBootstrap,
    network: Option<RecordedStorageNetwork>,
    started: Option<StartedBootstrapPair>,
    pending_pair: Option<PendingBootstrapPair>,
    failure: HostError,
) -> ScopedRuntimeOutcome {
    pending_with_initialization(prepared, network, started, pending_pair, None, failure)
}

fn pending_with_initialization(
    prepared: PreparedAcquiredBootstrap,
    network: Option<RecordedStorageNetwork>,
    started: Option<StartedBootstrapPair>,
    pending_pair: Option<PendingBootstrapPair>,
    initialization: Option<VerifiedStorageInitializationReady>,
    failure: HostError,
) -> ScopedRuntimeOutcome {
    ScopedRuntimeOutcome::Pending(PendingScopedBootstrap {
        prepared,
        network,
        started,
        pending_pair,
        initialization,
        failure,
    })
}
