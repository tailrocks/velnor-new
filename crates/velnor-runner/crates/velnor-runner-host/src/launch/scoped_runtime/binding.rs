use bollard::Docker;

use crate::{
    error::HostError,
    journal::{ConnectionJournal, Journal, RecordedStorageNetwork, VerifiedMetadataInitialization},
    launch::{
        SelectedEngineContext, VerifiedWorkerImages,
        bootstrap::{PreparedAcquiredBootstrap, StartedBootstrapPair},
        resource_probe::ImageProvider,
    },
    worker::storage_initialization::VerifiedStorageInitializationReady,
};

use super::{BoundScopedBootstrap, ScopedRuntimeOutcome, pending};

pub(super) async fn bind_started<P: ImageProvider>(
    prepared: PreparedAcquiredBootstrap,
    network: RecordedStorageNetwork,
    started: StartedBootstrapPair,
    metadata: VerifiedMetadataInitialization,
    initialization: VerifiedStorageInitializationReady,
    docker: &Docker,
    scoped: &ConnectionJournal,
    host: &Journal,
    provider: &P,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
) -> Result<ScopedRuntimeOutcome, HostError> {
    let binding = match crate::launch::bootstrap::observe_storage_resources_with_provider(
        docker,
        scoped,
        host,
        provider,
        engine,
        images,
        &prepared,
        &started,
        &initialization,
    )
    .await
    {
        Ok(binding) => binding,
        Err(error) => {
            return Ok(pending(prepared, Some(network), Some(started), None, error));
        }
    };
    let guest_boot_id = binding.guest_boot_id().to_owned();
    if let Err(error) = scoped
        .bind_storage_resources(prepared.acquired().reservation_id(), &binding)
        .await
    {
        return Ok(pending(prepared, Some(network), Some(started), None, error));
    }
    Ok(ScopedRuntimeOutcome::Bound(BoundScopedBootstrap {
        prepared,
        network,
        started,
        metadata,
        initialization,
        binding,
        guest_boot_id,
    }))
}
