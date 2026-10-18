//! Fresh selected-engine and worker-image identity checks.

use std::collections::HashMap;

use bollard::Docker;
use bollard::models::ImageInspect;

use crate::launch::{SelectedEngineContext, VerifiedImageIdentity, VerifiedWorkerImages};

use super::identity::{SelectedDaemon, inspect_image, selected_daemon_info};
use super::{ProviderFailure, valid_hex, valid_sha};

const IMAGE_INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
const IMAGE_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";

pub(super) async fn verify_worker_images_for_engine(
    docker: &Docker,
    expected: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
) -> Result<(), ProviderFailure> {
    if !expected.matches_images(images)
        || images.platform() != "linux/amd64"
        || !valid_sha(images.source_sha())
        || !valid_hex(images.docker_root_digest(), 64)
    {
        return Err(ProviderFailure::ImageIdentity);
    }
    let selected = SelectedDaemon {
        engine_id: expected.engine_id().to_owned(),
        docker_root_digest: expected.docker_root_digest().to_owned(),
        architecture: expected.daemon_architecture().to_owned(),
    };
    let observed = selected_daemon_info(docker, &selected).await?;
    if observed.engine_id != images.engine_id()
        || observed.docker_root_digest != images.docker_root_digest()
        || observed.architecture != images.daemon_architecture()
    {
        return Err(ProviderFailure::ImageIdentity);
    }
    verify_image(docker, images, "velnor-runner", images.runner()).await?;
    verify_image(docker, images, "velnor-dind", images.dind()).await?;
    let observed = selected_daemon_info(docker, &selected).await?;
    if observed.engine_id != images.engine_id()
        || observed.docker_root_digest != images.docker_root_digest()
        || observed.architecture != images.daemon_architecture()
    {
        return Err(ProviderFailure::ImageIdentity);
    }
    Ok(())
}

async fn verify_image(
    docker: &Docker,
    images: &VerifiedWorkerImages,
    image_name: &str,
    identity: &VerifiedImageIdentity,
) -> Result<(), ProviderFailure> {
    if !valid_hex(identity.archive_sha256(), 64)
        || !valid_hex(identity.archive_index_digest(), 64)
        || !valid_digest(identity.manifest_digest())
        || !valid_digest(identity.config_digest())
        || identity.manifest_size() == 0
        || identity.image_index_digest().is_some() != identity.image_index_size().is_some()
        || identity.image_index_size().is_some_and(|size| size == 0)
        || identity
            .image_index_digest()
            .is_some_and(|digest| !valid_digest(digest))
    {
        return Err(ProviderFailure::ImageIdentity);
    }
    let reference = format!(
        "{image_name}:v{}-{}",
        images.source_sha(),
        identity.archive_sha256()
    );
    let image = inspect_image(docker, identity.runtime_id())
        .await?
        .ok_or(ProviderFailure::ImageIdentity)?;
    verify_inspection(&image, identity, &reference, image_name)
}

fn verify_inspection(
    image: &ImageInspect,
    identity: &VerifiedImageIdentity,
    reference: &str,
    image_name: &str,
) -> Result<(), ProviderFailure> {
    let id = image.id.as_deref().ok_or(ProviderFailure::ImageIdentity)?;
    if id != identity.runtime_id()
        || image.os.as_deref() != Some("linux")
        || image.architecture.as_deref() != Some("amd64")
        || !image
            .repo_tags
            .as_ref()
            .is_some_and(|tags| tags.len() == 1 && accepted_local_tag(&tags[0], reference))
        || image.manifests.is_some()
    {
        return Err(ProviderFailure::ImageIdentity);
    }
    match image.descriptor.as_ref() {
        None if id == identity.config_digest()
            && image.repo_digests.as_ref().is_none_or(Vec::is_empty) => {}
        Some(descriptor) => {
            let (digest, media_type, size) = match identity.image_index_digest() {
                Some(index_digest) => (
                    index_digest,
                    IMAGE_INDEX_MEDIA_TYPE,
                    identity.image_index_size(),
                ),
                None => (
                    identity.manifest_digest(),
                    IMAGE_MANIFEST_MEDIA_TYPE,
                    Some(identity.manifest_size()),
                ),
            };
            let expected_annotations = HashMap::from([
                (
                    "io.containerd.image.name".to_owned(),
                    format!("docker.io/library/{reference}"),
                ),
                (
                    "org.opencontainers.image.ref.name".to_owned(),
                    reference
                        .split_once(':')
                        .map(|(_, tag)| tag.to_owned())
                        .ok_or(ProviderFailure::ImageIdentity)?,
                ),
            ]);
            if id != digest
                || descriptor.digest.as_deref() != Some(digest)
                || descriptor.media_type.as_deref() != Some(media_type)
                || descriptor.size.and_then(|value| u64::try_from(value).ok()) != size
                || descriptor.annotations.as_ref() != Some(&expected_annotations)
                || descriptor.urls.is_some()
                || descriptor.data.is_some()
                || descriptor.platform.is_some()
            {
                return Err(ProviderFailure::ImageIdentity);
            }
            if let Some(repo_digests) = image.repo_digests.as_ref()
                && !accepted_local_repo_digest(repo_digests, image_name, digest)
            {
                return Err(ProviderFailure::ImageIdentity);
            }
        }
        None => return Err(ProviderFailure::ImageIdentity),
    }
    Ok(())
}

fn accepted_local_tag(observed: &str, expected: &str) -> bool {
    observed == expected || observed == format!("docker.io/library/{expected}")
}

fn accepted_local_repo_digest(observed: &[String], image_name: &str, digest: &str) -> bool {
    observed.is_empty()
        || (observed.len() == 1
            && (observed[0] == format!("{image_name}@{digest}")
                || observed[0] == format!("docker.io/library/{image_name}@{digest}")))
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|value| valid_hex(value, 64))
}

#[cfg(test)]
mod tests;
