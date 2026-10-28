// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::models::ImageInspect;

use crate::error::HostError;

use super::deadline;
use super::probes::{self, DaemonIdentity, VersionEvidence};
use super::resources::OwnedResources;
use super::source::SourceSnapshot;

const IMAGE_TIMEOUT: Duration = Duration::from_secs(5);
const SOURCE_LABEL: &str = "org.opencontainers.image.revision";
const IMAGE_KIND_LABEL: &str = "io.velnor.image.kind";

pub(super) struct Qualification {
    pub(super) outer: DaemonIdentity,
    pub(super) inner_id: String,
    pub(super) versions: VersionEvidence,
}

pub(super) async fn qualify(
    docker: &Docker,
    source: &SourceSnapshot,
    runner_id: &str,
    dind_id: &str,
) -> Result<Qualification, HostError> {
    let mut resources = OwnedResources::new()?;
    let result = qualify_with_resources(docker, source, runner_id, dind_id, &mut resources).await;
    let cleanup = resources.cleanup(docker).await;
    match (result, cleanup) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), Ok(())) | (_, Err(error)) => Err(error),
    }
}

async fn qualify_with_resources(
    docker: &Docker,
    source: &SourceSnapshot,
    runner_id: &str,
    dind_id: &str,
    resources: &mut OwnedResources,
) -> Result<Qualification, HostError> {
    inspect_image(docker, source, runner_id, "runner").await?;
    inspect_image(docker, source, dind_id, "dind").await?;
    let outer = probes::inspect_outer(docker).await?;
    let versions = probes::verify_versions(docker, source, runner_id, dind_id, resources).await?;
    let socket_volume = resources.create_volume(docker, "socket").await?;
    let data_volume = resources.create_volume(docker, "data").await?;
    let dind_container = resources
        .start_dind(docker, dind_id, &socket_volume, &data_volume)
        .await?;
    let inner_id = probes::wait_inner_daemon(docker, &dind_container).await?;
    if inner_id == outer.id {
        return Err(HostError::Identity);
    }
    let runner_id_from_socket =
        probes::query_runner_daemon(docker, runner_id, &socket_volume, resources).await?;
    if runner_id_from_socket != inner_id {
        return Err(HostError::Identity);
    }
    inspect_image(docker, source, runner_id, "runner").await?;
    inspect_image(docker, source, dind_id, "dind").await?;
    let final_outer = probes::inspect_outer(docker).await?;
    if final_outer != outer {
        return Err(HostError::Identity);
    }
    Ok(Qualification {
        outer,
        inner_id,
        versions,
    })
}

async fn inspect_image(
    docker: &Docker,
    source: &SourceSnapshot,
    image_id: &str,
    expected_kind: &str,
) -> Result<(), HostError> {
    let image = deadline(docker.inspect_image(image_id), IMAGE_TIMEOUT)
        .await?
        .map_err(|error| {
            if is_missing(&error) {
                HostError::Identity
            } else {
                HostError::Docker
            }
        })?;
    validate_image(&image, source, image_id, expected_kind)
}

fn validate_image(
    image: &ImageInspect,
    source: &SourceSnapshot,
    image_id: &str,
    expected_kind: &str,
) -> Result<(), HostError> {
    if image.id.as_deref() != Some(image_id)
        || image.os.as_deref() != Some("linux")
        || image.architecture.as_deref() != Some("amd64")
        || image
            .variant
            .as_deref()
            .is_some_and(|variant| !variant.is_empty())
    {
        return Err(HostError::Identity);
    }
    let revision = image
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
        .and_then(|labels| labels.get(SOURCE_LABEL));
    let image_kind = image
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
        .and_then(|labels| labels.get(IMAGE_KIND_LABEL));
    if revision.map(String::as_str) != Some(source.source_sha.as_str())
        || image_kind.map(String::as_str) != Some(expected_kind)
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn is_missing(error: &DockerError) -> bool {
    matches!(
        error,
        DockerError::DockerResponseServerError {
            status_code: 404,
            ..
        }
    )
}
