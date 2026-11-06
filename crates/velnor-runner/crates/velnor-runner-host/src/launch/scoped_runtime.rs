// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod authorization;
mod binding;
mod channel_outcome;
mod jit;
mod preparation;
mod provider_entry;
mod registry;

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

pub(crate) use registry::RunnerReadyRegistry;

use bollard::Docker;
use std::{future::Future, pin::Pin, time::Instant};
use velnor_runner_github::Transport;

use crate::{
    error::HostError,
    journal::{ConnectionJournal, Journal, RecordedStorageNetwork, VerifiedMetadataInitialization},
    launch::{
        Lane, SelectedEngineContext, VerifiedWorkerImages,
        bootstrap::{PendingBootstrapPair, PreparedAcquiredBootstrap, StartedBootstrapPair},
        resource_probe::ImageProvider,
        steps::ScopedProvisioningOutcome,
    },
    worker::storage_initialization::VerifiedStorageInitializationReady,
};

#[must_use]
pub(crate) enum ScopedRuntimeOutcome {
    NoStart,
    Bound(Box<BoundScopedBootstrap>),
    Pending(Box<PendingScopedBootstrap>),
}

#[must_use]
pub(crate) enum ScopedChannelOutcome {
    NoStart,
    Ready(Box<ChannelReadyBootstrap>),
    PendingPreparation(Box<PendingScopedBootstrap>),
    PendingJit(PendingJitBootstrap),
    PendingChannel(PendingChannelBootstrap),
}

pub(crate) type ScopedPrepareFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ScopedRuntimeOutcome, HostError>> + 'a>>;

#[must_use]
pub(crate) struct BoundScopedBootstrap {
    prepared: PreparedAcquiredBootstrap,

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

    pub(crate) fn metadata(&self) -> &VerifiedMetadataInitialization {
        &self.metadata
    }

    pub(crate) fn initialization(&self) -> &VerifiedStorageInitializationReady {
        &self.initialization
    }

    pub(crate) fn guest_boot_id(&self) -> &str {
        &self.guest_boot_id
    }
}

#[must_use]
pub(crate) struct PendingScopedBootstrap {
    prepared: PreparedAcquiredBootstrap,

    _network: Option<RecordedStorageNetwork>,
    _started: Option<StartedBootstrapPair>,
    _pending_pair: Option<PendingBootstrapPair>,
    _initialization: Option<VerifiedStorageInitializationReady>,
}

impl PendingScopedBootstrap {
    pub(crate) fn prepared(&self) -> &PreparedAcquiredBootstrap {
        &self.prepared
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

pub(crate) async fn prepare_jit_and_measure_scoped<T>(
    outcome: ScopedProvisioningOutcome,
    context: &ScopedHostContext<'_>,
    lane: &mut T,
    admin_token: &str,
    deadline: Instant,
) -> Result<ScopedChannelOutcome, HostError>
where
    T: Transport + Lane,
{
    let ScopedHostContext {
        docker,
        host,
        scoped,
        secret: release_secret,
        engine,
        images,
    } = *context;
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
        ScopedRuntimeOutcome::Bound(bound) => *bound,
    };
    let jit = match create_and_bind(bound, lane, admin_token, scoped).await {
        Ok(jit) => jit,
        Err(pending) => return Ok(ScopedChannelOutcome::PendingJit(pending)),
    };
    match jit.measure_channel(docker, engine, images, deadline).await {
        Ok(ready) => Ok(ScopedChannelOutcome::Ready(Box::new(ready))),
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
    let _ = &failure;
    ScopedRuntimeOutcome::Pending(Box::new(PendingScopedBootstrap {
        prepared,
        _network: network,
        _started: started,
        _pending_pair: pending_pair,
        _initialization: initialization,
    }))
}
