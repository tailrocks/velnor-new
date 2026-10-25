use crate::{
    error::HostError,
    launch::bootstrap::{
        PreparedAcquiredBootstrap, StartedBootstrapPair, VerifiedStorageResourceBinding,
    },
    launch::resource_probe::ImageProvider,
    worker::storage_initialization::VerifiedStorageInitializationReady,
};

use super::StorageObservationContext;

pub(in crate::launch) async fn observe_storage_resources_with_provider<P: ImageProvider>(
    context: &StorageObservationContext<'_, P>,
    prepared: &PreparedAcquiredBootstrap,
    started: &StartedBootstrapPair,
    initialization: &VerifiedStorageInitializationReady,
) -> Result<VerifiedStorageResourceBinding, HostError> {
    super::observe_storage_resources_core(context, prepared, started, initialization).await
}
