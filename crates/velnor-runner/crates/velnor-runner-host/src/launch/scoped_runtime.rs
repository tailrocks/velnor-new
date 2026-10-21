//! Prepare one acquired job through measured storage binding.

mod authorization;
mod binding;
mod channel_outcome;
mod jit;
mod preparation;
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
pub(crate) use jit::{
    ChannelReadyBootstrap, PendingChannelBootstrap, PendingJitBootstrap, RunnerReadyBootstrap,
    create_and_bind,
};
use preparation::prepare_started_and_bound;
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
    // Retain the exact network proof with the bound bootstrap until its owner drops.
    _network: RecordedStorageNetwork,
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

    #[cfg(test)]
    pub(crate) fn started(&self) -> &StartedBootstrapPair {
        &self.started
    }

    pub(crate) fn metadata(&self) -> &VerifiedMetadataInitialization {
        &self.metadata
    }

    pub(crate) fn initialization(&self) -> &VerifiedStorageInitializationReady {
        &self.initialization
    }

    #[cfg(test)]
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
    // Keep every acquired handle and proof alive while terminal cleanup remains pending.
    _network: Option<RecordedStorageNetwork>,
    _started: Option<StartedBootstrapPair>,
    _pending_pair: Option<PendingBootstrapPair>,
    _initialization: Option<VerifiedStorageInitializationReady>,
    #[cfg(test)]
    failure: HostError,
}

impl PendingScopedBootstrap {
    pub(crate) fn prepared(&self) -> &PreparedAcquiredBootstrap {
        &self.prepared
    }

    #[cfg(test)]
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

impl<P> Copy for ScopedPrepareContext<'_, P> {}

impl<P> Clone for ScopedPrepareContext<'_, P> {
    fn clone(&self) -> Self {
        *self
    }
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
    #[cfg(not(test))]
    let _ = &failure;
    ScopedRuntimeOutcome::Pending(PendingScopedBootstrap {
        prepared,
        _network: network,
        _started: started,
        _pending_pair: pending_pair,
        _initialization: initialization,
        #[cfg(test)]
        failure,
    })
}
