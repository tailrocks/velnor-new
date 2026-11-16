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
    deadline: Instant,
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
        let reservation_id = prepared.acquired().reservation_id();
        let outcome = Box::pin(prepare_started_and_bound(prepared, context)).await?;
        if matches!(outcome, ScopedRuntimeOutcome::Pending(_))
            && let Err(error) = Box::pin(super::bootstrap::abandon_and_retire_unbound_pair_before(
                context.deadline,
                context.docker,
                context.scoped,
                context.engine,
                reservation_id,
            ))
            .await
        {
            eprintln!(
                "host controller kept an unbound pair charged reservation={reservation_id} reason={error}"
            );
        }
        Ok(outcome)
    })
}

fn scoped_prepare_context<'a, P: ImageProvider + 'a>(
    docker: &'a Docker,
    scoped: &'a ConnectionJournal,
    host: &'a Journal,
    provider: &'a P,
    engine: &'a SelectedEngineContext,
    images: &'a VerifiedWorkerImages,
    deadline: Instant,
) -> ScopedPrepareContext<'a, P> {
    ScopedPrepareContext {
        docker,
        scoped,
        host,
        provider,
        engine,
        images,
        deadline,
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
        scoped,
        engine,
        images,
        ..
    } = *context;
    let bound = match prepare_and_bind_scoped(outcome, context, deadline).await? {
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

fn held_preparation_line(reservation_id: i64, failure: HostError) -> String {
    format!("host controller pair preparation held reservation={reservation_id} reason={failure}")
}

fn cleanup_failure_line(reservation_id: i64, failure: HostError) -> String {
    format!("host controller pair cleanup failed reservation={reservation_id} reason={failure}")
}

fn pending_with_initialization(
    prepared: PreparedAcquiredBootstrap,
    network: Option<RecordedStorageNetwork>,
    started: Option<StartedBootstrapPair>,
    pending_pair: Option<PendingBootstrapPair>,
    initialization: Option<VerifiedStorageInitializationReady>,
    failure: HostError,
) -> ScopedRuntimeOutcome {
    eprintln!(
        "{}",
        held_preparation_line(prepared.acquired().reservation_id(), failure)
    );
    ScopedRuntimeOutcome::Pending(Box::new(PendingScopedBootstrap {
        prepared,
        _network: network,
        _started: started,
        _pending_pair: pending_pair,
        _initialization: initialization,
    }))
}

#[cfg(test)]
mod tests {
    use super::{cleanup_failure_line, held_preparation_line};
    use crate::error::{ErrorDetail, HostError};

    #[test]
    fn the_held_line_names_the_reservation_and_the_real_reason() {
        let failure = HostError::Helper(ErrorDetail::from_bytes(b"probe_failed"));
        let line = held_preparation_line(7, failure);
        assert!(line.contains("reservation=7"), "{line}");
        assert!(line.contains("probe_failed"), "{line}");
    }

    #[test]
    fn the_cleanup_line_names_the_reservation_and_the_cleanup_reason() {
        let failure = HostError::Helper(ErrorDetail::from_bytes(b"cleanup_probe_failed"));
        let line = cleanup_failure_line(9, failure);
        assert!(line.contains("reservation=9"), "{line}");
        assert!(line.contains("cleanup_probe_failed"), "{line}");
    }
}
