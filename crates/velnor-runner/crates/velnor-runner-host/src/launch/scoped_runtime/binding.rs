use super::{
    BoundScopedBootstrap, HostError, ImageProvider, PendingBootstrapPair,
    PreparedAcquiredBootstrap, RecordedStorageNetwork, ScopedPrepareContext, ScopedRuntimeOutcome,
    StartedBootstrapPair, VerifiedMetadataInitialization, VerifiedStorageInitializationReady,
};

pub(super) async fn cleanup_failed_pair<P: ImageProvider>(
    prepared: PreparedAcquiredBootstrap,
    network: RecordedStorageNetwork,
    error: HostError,
    pending_pair: Option<PendingBootstrapPair>,
    reservation_id: i64,
    context: ScopedPrepareContext<'_, P>,
) -> ScopedRuntimeOutcome {
    let cleaned = Box::pin(
        super::super::bootstrap::remove_bootstrap_after_terminal_failure(
            context.docker,
            context.scoped,
            context.host,
            context.engine,
            context.images,
            reservation_id,
            context.provider,
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
    context: ScopedPrepareContext<'_, P>,
) -> Result<ScopedRuntimeOutcome, HostError> {
    let observation_context = super::super::bootstrap::StorageObservationContext::new(
        context.docker,
        context.scoped,
        context.host,
        context.provider,
        context.engine,
        context.images,
    );
    let binding = match super::super::bootstrap::observe_storage_resources_with_provider(
        &observation_context,
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
    if let Err(error) = context
        .scoped
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
        _network: network,
        started,
        metadata,
        initialization,
        binding,
        guest_boot_id,
    }))
}
