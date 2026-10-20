use bollard::Docker;

use crate::{
    error::HostError,
    journal::{ConnectionJournal, Journal},
    launch::{
        SelectedEngineContext, VerifiedWorkerImages,
        bootstrap::{
            PreparedAcquiredBootstrap, StartedBootstrapPair, VerifiedStorageResourceBinding,
        },
        resource_probe::ImageProvider,
    },
    worker::storage_initialization::VerifiedStorageInitializationReady,
};

pub(in crate::launch) async fn observe_storage_resources_with_provider<P: ImageProvider>(
    docker: &Docker,
    journal: &ConnectionJournal,
    host_journal: &Journal,
    provider: &P,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    prepared: &PreparedAcquiredBootstrap,
    started: &StartedBootstrapPair,
    initialization: &VerifiedStorageInitializationReady,
) -> Result<VerifiedStorageResourceBinding, HostError> {
    super::observe_storage_resources_core(
        docker,
        journal,
        host_journal,
        provider,
        engine,
        images,
        prepared,
        started,
        initialization,
    )
    .await
}
