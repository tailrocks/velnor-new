// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::models::ImageInspect;

use crate::error::HostError;

use super::deadline;
use super::probes::{self, DaemonIdentity};
use super::resources::OwnedResources;
use super::source::SourceSnapshot;
use super::tool_versions::{self, VersionEvidence};

const IMAGE_TIMEOUT: Duration = Duration::from_secs(5);
const SOURCE_LABEL: &str = "org.opencontainers.image.revision";
const IMAGE_KIND_LABEL: &str = "io.velnor.image.kind";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum IdentityKind {
    Config,
    PlatformManifest,
}

pub(super) struct Qualification {
    pub(super) outer: DaemonIdentity,
    pub(super) inner_id: String,
    pub(super) versions: VersionEvidence,
    pub(super) runner_identity: IdentityKind,
    pub(super) dind_identity: IdentityKind,
    pub(super) probe_identity: IdentityKind,
}

pub(super) async fn qualify(
    docker: &Docker,
    source: &SourceSnapshot,
    runner_id: &str,
    dind_id: &str,
    probe_id: &str,
) -> Result<Qualification, HostError> {
    let mut resources = OwnedResources::new()?;
    let result =
        qualify_with_resources(docker, source, runner_id, dind_id, probe_id, &mut resources).await;
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
    probe_id: &str,
    resources: &mut OwnedResources,
) -> Result<Qualification, HostError> {
    let runner_identity = inspect_image(docker, source, runner_id, "runner").await?;
    let dind_identity = inspect_image(docker, source, dind_id, "dind").await?;
    let probe_identity = inspect_image(docker, source, probe_id, "resource-probe").await?;
    let outer = probes::inspect_outer(docker).await?;
    let versions =
        tool_versions::verify_versions(docker, source, runner_id, dind_id, resources).await?;
    let output = probes::run_command(
        docker,
        resources,
        probes::CommandSpec {
            image_id: probe_id,
            role: "resource-probe",
            entrypoint: "/velnor/resource-probe",
            command: &[],
            socket_volume: None,
            timeout: IMAGE_TIMEOUT,
            probe_root: Some(&outer.root),
        },
    )
    .await?;
    super::super::smoke::validate_bytes(&output)?;
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
    if inspect_image(docker, source, runner_id, "runner").await? != runner_identity
        || inspect_image(docker, source, dind_id, "dind").await? != dind_identity
        || inspect_image(docker, source, probe_id, "resource-probe").await? != probe_identity
    {
        return Err(HostError::Identity);
    }
    let final_outer = probes::inspect_outer(docker).await?;
    if final_outer != outer {
        return Err(HostError::Identity);
    }
    Ok(Qualification {
        outer,
        inner_id,
        versions,
        runner_identity,
        dind_identity,
        probe_identity,
    })
}

async fn inspect_image(
    docker: &Docker,
    source: &SourceSnapshot,
    image_id: &str,
    expected_kind: &str,
) -> Result<IdentityKind, HostError> {
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
) -> Result<IdentityKind, HostError> {
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
    let identity = match image.descriptor.as_ref() {
        Some(descriptor) => {
            if descriptor.digest.as_deref() != Some(image_id)
                || descriptor.size.is_some_and(|size| size <= 0)
                || !matches!(
                    descriptor.media_type.as_deref(),
                    Some(
                        "application/vnd.oci.image.manifest.v1+json"
                            | "application/vnd.docker.distribution.manifest.v2+json"
                    )
                )
            {
                return Err(HostError::Identity);
            }
            if let Some(platform) = descriptor.platform.as_ref()
                && (platform.os.as_deref() != Some("linux")
                    || platform.architecture.as_deref() != Some("amd64")
                    || platform
                        .variant
                        .as_deref()
                        .is_some_and(|value| !value.is_empty())
                    || platform.os_version.is_some()
                    || platform.os_features.is_some())
            {
                return Err(HostError::Identity);
            }
            IdentityKind::PlatformManifest
        }
        None => IdentityKind::Config,
    };
    let config = image.config.as_ref().ok_or(HostError::Identity)?;
    let (expected_user, expected_entrypoint) = match expected_kind {
        "runner" => ("runner", "/usr/local/bin/velnor-runner-entrypoint"),
        "dind" => ("", "/usr/local/bin/velnor-dind-entrypoint"),
        "resource-probe" => ("65532:65532", "/velnor/resource-probe"),
        _ => return Err(HostError::Identity),
    };
    if config.user.as_deref().unwrap_or("") != expected_user
        || config.entrypoint.as_deref() != Some(&[expected_entrypoint.to_owned()][..])
    {
        return Err(HostError::Identity);
    }
    let revision = config
        .labels
        .as_ref()
        .and_then(|labels| labels.get(SOURCE_LABEL));
    let image_kind = config
        .labels
        .as_ref()
        .and_then(|labels| labels.get(IMAGE_KIND_LABEL));
    if revision.map(String::as_str) != Some(source.source_sha.as_str())
        || (expected_kind != "resource-probe"
            && image_kind.map(String::as_str) != Some(expected_kind))
        || (expected_kind == "resource-probe" && config.working_dir.as_deref() != Some("/"))
    {
        return Err(HostError::Identity);
    }
    Ok(identity)
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
