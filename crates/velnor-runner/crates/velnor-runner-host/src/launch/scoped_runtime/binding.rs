use super::{
    BoundScopedBootstrap, ConnectionJournal, Docker, HostError, ImageProvider, Journal,
    PendingBootstrapPair, PreparedAcquiredBootstrap, RecordedStorageNetwork, ScopedRuntimeOutcome,
    SelectedEngineContext, StartedBootstrapPair, VerifiedMetadataInitialization,
    VerifiedStorageInitializationReady, VerifiedWorkerImages,
};

pub(super) async fn cleanup_failed_pair<P: ImageProvider>(
    prepared: PreparedAcquiredBootstrap,
    network: RecordedStorageNetwork,
    error: HostError,
    pending_pair: Option<PendingBootstrapPair>,
    docker: &Docker,
    scoped: &ConnectionJournal,
    host: &Journal,
    provider: &P,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    reservation_id: i64,
) -> ScopedRuntimeOutcome {
    let cleaned = Box::pin(
        super::super::bootstrap::remove_bootstrap_after_terminal_failure(
            docker,
            scoped,
            host,
            engine,
            images,
            reservation_id,
            provider,
        ),
    )
    .await
    .unwrap_or(false);
    super::pending(
        prepared,
        Some(network),
        None,
        if cleaned { None } else { pending_pair },
        error,
    )
}

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
    let binding = match super::super::bootstrap::observe_storage_resources_with_provider(
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
            return Ok(super::pending(
                prepared,
                Some(network),
                Some(started),
                None,
                error,
            ));
        }
    };
    let guest_boot_id = binding.guest_boot_id().to_owned();
    if let Err(error) = scoped
        .bind_storage_resources(prepared.acquired().reservation_id(), &binding)
        .await
    {
        return Ok(super::pending(
            prepared,
            Some(network),
            Some(started),
            None,
            error,
        ));
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
