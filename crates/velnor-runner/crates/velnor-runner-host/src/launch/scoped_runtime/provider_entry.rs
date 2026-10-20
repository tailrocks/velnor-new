#[cfg(test)]
use crate::launch::resource_probe::ImageProvider;

use bollard::Docker;

use crate::{
    error::HostError,
    journal::{ConnectionJournal, Journal},
    launch::{
        SelectedEngineContext, VerifiedWorkerImages, resource_probe::ReleaseReadSecret,
        steps::ScopedProvisioningOutcome,
    },
};

use super::{ScopedRuntimeOutcome, prepare_and_bind_scoped_with_provider};

/// Prepare storage, start both containers, and bind measured resources.
pub(crate) async fn prepare_and_bind_scoped(
    outcome: ScopedProvisioningOutcome,
    docker: &Docker,
    scoped: &ConnectionJournal,
    host: &Journal,
    release_secret: &ReleaseReadSecret,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
) -> Result<ScopedRuntimeOutcome, HostError> {
    let provider = crate::launch::resource_probe::release_image_provider(release_secret)?;
    prepare_and_bind_scoped_with_provider(outcome, docker, scoped, host, &provider, engine, images)
        .await
}

#[cfg(test)]
pub(in crate::launch) async fn prepare_and_bind_scoped_with_test_provider<P: ImageProvider>(
    outcome: ScopedProvisioningOutcome,
    docker: &Docker,
    scoped: &ConnectionJournal,
    host: &Journal,
    provider: &P,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
) -> Result<ScopedRuntimeOutcome, HostError> {
    prepare_and_bind_scoped_with_provider(outcome, docker, scoped, host, provider, engine, images)
        .await
}
