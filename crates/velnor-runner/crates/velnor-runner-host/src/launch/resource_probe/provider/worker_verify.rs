// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;

use crate::launch::verified_images::{RegistryWorkerImage, WorkerImageOrigin};
use crate::launch::{SelectedEngineContext, VerifiedWorkerImages};

use super::identity::{SelectedDaemon, selected_daemon_info};
use super::{ProviderFailure, valid_hex, valid_sha};

pub(super) async fn verify_worker_images_for_engine(
    docker: &Docker,
    expected: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
) -> Result<(), ProviderFailure> {
    let registry = images.registry_manifest_sha256().is_some();
    if !expected.matches_images(images)
        || (registry && images.platform() != "linux/amd64")
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
    for (identity, role) in [
        (images.runner(), super::WorkerImage::Runner),
        (images.dind(), super::WorkerImage::Dind),
    ] {
        match identity.origin() {
            WorkerImageOrigin::Registry(image) => {
                verify_image(expected.endpoint(), &selected, image).await?;
            }
            WorkerImageOrigin::Developer(image) => {
                super::developer::verify_worker_image(docker, images, identity, image, role)
                    .await?;
            }
        }
    }
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
    endpoint: &str,
    selected: &SelectedDaemon,
    image: &RegistryWorkerImage,
) -> Result<(), ProviderFailure> {
    let observed = super::registry_import::reinspect_bound(
        endpoint,
        selected,
        &image.identity,
        &image.projection,
    )
    .await?;
    if observed == image.observation {
        Ok(())
    } else {
        Err(ProviderFailure::ImageIdentity)
    }
}
