// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::models::ImageInspect;

use crate::error::QualificationError as HostError;

use super::deadline;
use super::probes::{self, DaemonIdentity};
use super::resources::OwnedResources;
use super::tool_versions::{self, VersionEvidence};
use crate::SourceIdentity;

const IMAGE_TIMEOUT: Duration = Duration::from_secs(5);
const SOURCE_LABEL: &str = "org.opencontainers.image.revision";
const IMAGE_KIND_LABEL: &str = "io.velnor.image.kind";

#[derive(Debug)]
pub struct Qualification {
    pub outer: DaemonIdentity,
    pub inner_id: String,
    pub versions: VersionEvidence,
}

pub async fn qualify(
    docker: &Docker,
    source: &SourceIdentity,
    runner_id: &str,
    dind_id: &str,
    healthchecks: [Option<bollard::models::HealthConfig>; 2],
    end: std::time::Instant,
) -> Result<Qualification, HostError> {
    let [runner_health, dind_health] = healthchecks;
    let mut resources = OwnedResources::new(&[(runner_id, runner_health), (dind_id, dind_health)])?;
    let result = match super::remaining(end) {
        Ok(limit) => super::deadline(
            qualify_with_resources(docker, source, runner_id, dind_id, &mut resources),
            limit,
        )
        .await
        .and_then(std::convert::identity),
        Err(error) => Err(error),
    };
    let cleanup = resources.cleanup(docker).await;
    match (result, cleanup) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), Ok(())) | (_, Err(error)) => Err(error),
    }
}

pub struct QualificationSession {
    docker: Docker,
    daemon: DaemonIdentity,
    root: String,
    resources: OwnedResources,
}

impl std::fmt::Debug for QualificationSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("QualificationSession")
            .finish_non_exhaustive()
    }
}

pub async fn open_qualification_session(
    docker: Docker,
    journal_path: PathBuf,
    root: String,
) -> Result<QualificationSession, HostError> {
    tokio::spawn(async move {
        let daemon = probes::inspect_outer(&docker).await?;
        if daemon.root != root {
            return Err(HostError::Identity);
        }
        let mut resources = OwnedResources::journaled(&[], &journal_path, &daemon)?;
        if probes::inspect_outer(&docker).await? != daemon {
            return Err(HostError::Identity);
        }
        super::deadline(resources.cleanup(&docker), Duration::from_secs(600)).await??;
        resources.reset()?;
        Ok(QualificationSession {
            docker,
            daemon,
            root,
            resources,
        })
    })
    .await
    .map_err(|_| HostError::Docker)?
}

impl QualificationSession {
    pub async fn qualify(
        self,
        source: SourceIdentity,
        runner_id: String,
        dind_id: String,
        probe_id: String,
        healthchecks: [Option<bollard::models::HealthConfig>; 3],
        end: Instant,
    ) -> Result<Qualification, HostError> {
        tokio::spawn(async move {
            let Self {
                docker,
                daemon,
                root,
                mut resources,
            } = self;
            let [runner_health, dind_health, probe_health] = healthchecks;
            resources.set_healthchecks(&[
                (&runner_id, runner_health),
                (&dind_id, dind_health),
                (&probe_id, probe_health),
            ]);
            let result = match super::remaining(end) {
                Ok(limit) => Box::pin(super::deadline(
                    async {
                        if probes::inspect_outer(&docker).await? != daemon {
                            return Err(HostError::Identity);
                        }
                        let result = qualify_with_resources(
                            &docker,
                            &source,
                            &runner_id,
                            &dind_id,
                            &mut resources,
                        )
                        .await?;
                        super::probe_image::run(&docker, &probe_id, &root, &mut resources).await?;
                        if probes::inspect_outer(&docker).await? != daemon {
                            return Err(HostError::Identity);
                        }
                        Ok(result)
                    },
                    limit,
                ))
                .await
                .and_then(std::convert::identity),
                Err(error) => Err(error),
            };
            let cleanup = if probes::inspect_outer(&docker).await? == daemon {
                super::deadline(resources.cleanup(&docker), Duration::from_secs(600)).await?
            } else {
                Err(HostError::Identity)
            };
            match (result, cleanup) {
                (Ok(result), Ok(())) => Ok(result),
                (Err(error), Ok(())) | (_, Err(error)) => Err(error),
            }
        })
        .await
        .map_err(|_| HostError::Docker)?
    }
}

async fn qualify_with_resources(
    docker: &Docker,
    source: &SourceIdentity,
    runner_id: &str,
    dind_id: &str,
    resources: &mut OwnedResources,
) -> Result<Qualification, HostError> {
    inspect_image(docker, source, runner_id, "runner").await?;
    inspect_image(docker, source, dind_id, "dind").await?;
    let outer = probes::inspect_outer(docker).await?;
    let versions =
        tool_versions::verify_versions(docker, source, runner_id, dind_id, resources).await?;
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
    source: &SourceIdentity,
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
    source: &SourceIdentity,
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
    let config = image.config.as_ref().ok_or(HostError::Identity)?;
    let (expected_user, expected_entrypoint) = match expected_kind {
        "runner" => ("runner", "/usr/local/bin/velnor-runner-entrypoint"),
        "dind" => ("", "/usr/local/bin/velnor-dind-entrypoint"),
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
