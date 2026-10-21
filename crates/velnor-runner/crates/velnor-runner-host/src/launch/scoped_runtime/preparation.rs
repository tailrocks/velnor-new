use bollard::Docker;

use super::{
    ScopedRuntimeOutcome,
    binding::{bind_started, cleanup_failed_pair},
    pending, pending_with_initialization,
};
use crate::{
    error::HostError,
    journal::{ConnectionJournal, Journal, RecordedStorageNetwork},
    launch::{
        SelectedEngineContext, VerifiedWorkerImages,
        bootstrap::{self, PreparedAcquiredBootstrap, StartedBootstrapPair},
        resource_probe::ImageProvider,
    },
};

pub(super) async fn prepare_started_and_bound<P: ImageProvider>(
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
        Err(pending) => return Ok(*pending),
    };
    let context = super::scoped_prepare_context(docker, scoped, host, provider, engine, images);
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
        context,
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
) -> Result<StartedPairPreparation, Box<ScopedRuntimeOutcome>> {
    let reservation_id = prepared.acquired().reservation_id();
    let Some(initializer) = prepared.storage().pending_initialization() else {
        return Err(Box::new(pending(
            prepared,
            None,
            None,
            None,
            HostError::Journal,
        )));
    };
    let network = match Box::pin(bootstrap::create_owned_network(
        docker,
        scoped,
        engine,
        reservation_id,
    ))
    .await
    {
        Ok(network) => network,
        Err(error) => return Err(Box::new(pending(prepared, None, None, None, error))),
    };
    let started = match Box::pin(bootstrap::start_bootstrap_pair(
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
            return Err(Box::new(
                cleanup_failed_pair(
                    prepared,
                    network,
                    error,
                    pending_pair,
                    reservation_id,
                    super::scoped_prepare_context(docker, scoped, host, provider, engine, images),
                )
                .await,
            ));
        }
    };
    Ok(StartedPairPreparation {
        prepared,
        network,
        started,
    })
}
