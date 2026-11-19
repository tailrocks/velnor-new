// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;
use bollard::models::{ContainerCreateBody, HostConfig, Mount, MountType};
use bollard::query_parameters::{
    CreateContainerOptions, StartContainerOptions, UploadToContainerOptions, WaitContainerOptions,
};
use futures_util::StreamExt;

use crate::config::RunnerPlatform;
use crate::docker_client::docker_deadline;
use crate::error::HostError;

use super::layout;
use super::pair::owned_labels;
use super::remove::remove_container;
use super::request::LocalPairRequest;

pub(super) enum MetadataUpload<'a> {
    Initialize(Vec<u8>),
    Control { name: &'a str, bytes: &'a [u8] },
}

struct Publication {
    purpose: String,
    archive: Vec<u8>,
    command: Option<Vec<String>>,
}

impl Publication {
    fn new(upload: MetadataUpload<'_>) -> Result<Self, HostError> {
        match upload {
            MetadataUpload::Initialize(archive) => Ok(Self {
                purpose: "init".to_owned(),
                archive,
                command: None,
            }),
            MetadataUpload::Control { name, bytes } => {
                let directory = format!("{}/source-proof/control", layout::METADATA_TARGET);
                let temporary = format!(".{name}.{}.tmp", uuid::Uuid::new_v4());
                Ok(Self {
                    purpose: name.trim_end_matches(".json").to_owned(),
                    archive: layout::control_archive(&temporary, bytes)?,
                    command: Some(vec![
                        "/bin/sh".to_owned(),
                        "-ec".to_owned(),
                        "sync \"$1\"; mv -f -- \"$1\" \"$2\"; sync \"$3\"".to_owned(),
                        "velnor-local-publish".to_owned(),
                        format!("{directory}/{temporary}"),
                        format!("{directory}/{name}"),
                        directory,
                    ]),
                })
            }
        }
    }
}

pub(super) async fn upload_to_metadata(
    docker: &Docker,
    request: &LocalPairRequest,
    pair: &str,
    volume: &str,
    contents: MetadataUpload<'_>,
) -> Result<(), HostError> {
    let publication = Publication::new(contents)?;
    let platform = RunnerPlatform::parse(&request.platform).ok_or(HostError::Config)?;
    let options = CreateContainerOptions {
        name: Some(format!(
            "{pair}-{}-{}",
            publication.purpose,
            uuid::Uuid::new_v4()
        )),
        platform: platform.as_str().to_owned(),
    };
    let config = helper(request, pair, volume, &publication);
    let created = docker_deadline(docker.create_container(Some(options), config))
        .await?
        .map_err(|_| HostError::Docker)?;
    let uploaded = upload(docker, &created.id, publication).await;
    let removed = remove_container(docker, &created.id).await;
    uploaded.and(removed)
}

fn helper(
    request: &LocalPairRequest,
    pair: &str,
    volume: &str,
    publication: &Publication,
) -> ContainerCreateBody {
    ContainerCreateBody {
        image: Some(request.dind_image.clone()),
        entrypoint: Some(
            publication
                .command
                .clone()
                .unwrap_or_else(|| vec!["/velnor-local-never-started".to_owned()]),
        ),
        cmd: Some(Vec::new()),
        network_disabled: Some(true),
        labels: Some(owned_labels(request, pair, &publication.purpose)),
        host_config: Some(HostConfig {
            network_mode: Some("none".to_owned()),
            mounts: Some(vec![Mount {
                target: Some(layout::METADATA_TARGET.to_owned()),
                source: Some(volume.to_owned()),
                typ: Some(MountType::VOLUME),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

async fn upload(docker: &Docker, helper: &str, publication: Publication) -> Result<(), HostError> {
    docker_deadline(docker.upload_to_container(
        helper,
        Some(UploadToContainerOptions {
            path: layout::METADATA_PARENT.to_owned(),
            ..Default::default()
        }),
        bollard::body_full(publication.archive.into()),
    ))
    .await?
    .map_err(|_| HostError::Docker)?;
    if publication.command.is_some() {
        publish(docker, helper).await?;
    }
    Ok(())
}

async fn publish(docker: &Docker, helper: &str) -> Result<(), HostError> {
    docker_deadline(docker.start_container(helper, None::<StartContainerOptions>))
        .await?
        .map_err(|_| HostError::Docker)?;
    let mut exited = docker.wait_container(
        helper,
        Some(WaitContainerOptions {
            condition: "not-running".to_owned(),
        }),
    );
    let result = docker_deadline(exited.next())
        .await?
        .ok_or(HostError::Docker)?
        .map_err(|_| HostError::Docker)?;
    if result.status_code != 0 || result.error.is_some() {
        return Err(HostError::Docker);
    }
    Ok(())
}
