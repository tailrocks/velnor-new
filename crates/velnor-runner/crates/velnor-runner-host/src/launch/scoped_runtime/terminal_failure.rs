use bollard::Docker;

use crate::{
    journal::{ConnectionJournal, Journal},
    launch::{SelectedEngineContext, VerifiedWorkerImages, resource_probe::ImageProvider},
};

pub(super) async fn remove_bootstrap_after_terminal_failure(
    docker: &Docker,
    scoped: &ConnectionJournal,
    host: &Journal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    reservation_id: i64,
    provider: &impl ImageProvider,
) -> bool {
    crate::launch::bootstrap::remove_bootstrap_after_terminal_failure(
        docker,
        scoped,
        host,
        engine,
        images,
        reservation_id,
        provider,
    )
    .await
    .unwrap_or(false)
}
