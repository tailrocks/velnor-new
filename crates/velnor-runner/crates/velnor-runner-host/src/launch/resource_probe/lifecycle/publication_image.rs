use bollard::Docker;

use crate::error::HostError;
use crate::journal::Journal;
use crate::launch::SelectedEngineContext;

use super::super::projection::VerifiedProbeImage;
use super::super::provider::{ImageProvider, ProbeImagePreparationEvidence};
use super::{Deadline, bind_info, info, same_guest, verified_image};

pub(in crate::launch::resource_probe) async fn verify_publication_image<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
    expected: &SelectedEngineContext,
) -> Result<(VerifiedProbeImage, ProbeImagePreparationEvidence), HostError> {
    verify_publication_image_with_deadline(docker, journal, provider, expected, Deadline::new())
        .await
}

pub(in crate::launch::resource_probe) async fn verify_publication_image_until<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
    expected: &SelectedEngineContext,
    deadline: std::time::Instant,
) -> Result<(VerifiedProbeImage, ProbeImagePreparationEvidence), HostError> {
    verify_publication_image_with_deadline(
        docker,
        journal,
        provider,
        expected,
        Deadline::until(deadline),
    )
    .await
}

async fn verify_publication_image_with_deadline<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
    expected: &SelectedEngineContext,
    deadline: Deadline,
) -> Result<(VerifiedProbeImage, ProbeImagePreparationEvidence), HostError> {
    let before = info(docker, &deadline).await?;
    let (engine_id, root, _, _) = bind_info(&before, journal, &deadline).await?;
    let architecture = super::super::provider::daemon_architecture(&before)
        .ok_or(HostError::Identity)?
        .to_owned();
    if engine_id != expected.engine_id()
        || root.digest() != expected.docker_root_digest()
        || architecture != expected.daemon_architecture()
        || expected.platform() != "linux/amd64"
    {
        return Err(HostError::Identity);
    }
    let candidate = provider
        .verified_candidate(docker, journal, &engine_id, &before)
        .await?
        .ok_or(HostError::Docker)?;
    if candidate.engine_id != engine_id
        || candidate.docker_root_digest != root.digest()
        || candidate.daemon_architecture != architecture
    {
        return Err(HostError::Identity);
    }
    let evidence = candidate.startup_evidence().ok_or(HostError::Identity)?;
    let image = verified_image(candidate)?;
    let after = info(docker, &deadline).await?;
    if !same_guest(&after, &engine_id, &root, &architecture)? {
        return Err(HostError::Identity);
    }
    Ok((image, evidence))
}
