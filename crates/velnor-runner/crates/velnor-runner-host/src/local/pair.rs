// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use bollard::Docker;
use bollard::models::VolumeCreateRequest;
use bollard::query_parameters::{
    InspectContainerOptions, StartContainerOptions, UploadToContainerOptions, WaitContainerOptions,
};
use futures_util::StreamExt;

use crate::config::RunnerPlatform;
use crate::docker_client::docker_deadline;
use crate::error::HostError;
use crate::journal::StorageRole;
use crate::launch::BootstrapContainerRole;
use crate::launch::bootstrap::containers::pair::{PairCreate, PairSpecContext, create_body};
use crate::worker::{WorkerContainerRole, WorkerIdentityNames, WorkerVolumeRole};

use super::io::{MetadataUpload, upload_to_metadata};
use super::layout;
use super::remove::{Removed, remove_labelled};
use super::request::{LocalPairRequest, Validated, validate};
use super::storage::{self, Sizes};

pub const ENVIRONMENT_LABEL: &str = "velnor-local";
pub(super) const PAIR_LABEL: &str = "velnor-local.pair";
const CRATES_MIRROR_OFF: &str = "VELNOR_CRATES_MIRROR=off";
const ROLE_LABEL: &str = "velnor-local.role";
pub const START_GRACE: std::time::Duration = std::time::Duration::from_millis(250);
pub(super) const SIDE_LABEL: &str = "velnor-local.side";
pub(super) const AGENT_LABEL: &str = "velnor-local.agent";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPair {
    request: LocalPairRequest,
    name: String,
    dind_id: String,
    runner_id: String,
    metadata_volume: String,
}

impl LocalPair {
    #[must_use]
    pub fn environment(&self) -> &str {
        &self.request.environment
    }

    pub(super) fn request(&self) -> &LocalPairRequest {
        &self.request
    }

    pub(super) fn metadata_volume(&self) -> &str {
        &self.metadata_volume
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn dind_id(&self) -> &str {
        &self.dind_id
    }

    #[must_use]
    pub fn runner_id(&self) -> &str {
        &self.runner_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{error}")]
pub struct StartFailure {
    pub error: HostError,
    pub leftover: Option<String>,
}

impl From<HostError> for StartFailure {
    fn from(error: HostError) -> Self {
        Self {
            error,
            leftover: None,
        }
    }
}

pub async fn start_pair(
    docker: &Docker,
    request: &LocalPairRequest,
) -> Result<LocalPair, StartFailure> {
    let validated = validate(request)?;
    let pair = validated.pair.clone();
    match Box::pin(create_and_start(docker, request, &validated)).await {
        Ok((dind_id, runner_id)) => Ok(LocalPair {
            request: request.clone(),
            name: pair,
            dind_id,
            runner_id,
            metadata_volume: validated
                .names
                .volume_name(WorkerVolumeRole::RunnerMetadata),
        }),
        Err(error) => {
            let leftover = remove_pair_named(docker, &pair).await.err().map(|_| pair);
            Err(StartFailure { error, leftover })
        }
    }
}

pub async fn remove_pair(docker: &Docker, pair: &LocalPair) -> Result<Removed, HostError> {
    remove_pair_named(docker, &pair.name).await
}

pub async fn remove_pair_named(docker: &Docker, name: &str) -> Result<Removed, HostError> {
    remove_labelled(docker, &[format!("{PAIR_LABEL}={name}")]).await
}

async fn create_and_start(
    docker: &Docker,
    request: &LocalPairRequest,
    validated: &Validated,
) -> Result<(String, String), HostError> {
    let names = &validated.names;
    let labels = |role: &str| owned_labels(request, &validated.pair, role);
    for role in WorkerVolumeRole::ALL {
        create_volume(
            docker,
            names.volume_name(role),
            role,
            labels(volume_role(role)),
            &validated.sizes,
        )
        .await?;
    }
    prefill_metadata(docker, request, validated).await?;
    let plan = storage::plan(request.sequence, &validated.sizes);
    let context = PairSpecContext {
        plan: &plan,
        names,
        connection_id: ENVIRONMENT_LABEL,
        budget: validated.budget,
    };
    let mut dind = create_body(
        names.container_name(WorkerContainerRole::Dind),
        request.dind_image.clone(),
        BootstrapContainerRole::Dind,
        format!("velnor-{}-private", names.remote_runner_name()?),
        validated.platform,
        &context,
    )?;
    own(&mut dind, labels("dind"), Some(&request.network))?;
    let dind_id = create(docker, dind).await?;
    start(docker, &dind_id).await?;
    upload(
        docker,
        &dind_id,
        layout::HOME_TARGET,
        layout::home_archive()?,
    )
    .await?;
    let runner = runner_create(
        names,
        request.runner_image.clone(),
        &dind_id,
        validated.platform,
        &context,
        labels("runner"),
    )?;
    let runner_id = create(docker, runner).await?;
    start(docker, &runner_id).await?;
    Ok((dind_id, runner_id))
}

fn runner_create(
    names: &WorkerIdentityNames,
    image: String,
    dind_id: &str,
    platform: RunnerPlatform,
    context: &PairSpecContext<'_>,
    labels: HashMap<String, String>,
) -> Result<PairCreate, HostError> {
    let mut runner = create_body(
        names.container_name(WorkerContainerRole::Runner),
        image,
        BootstrapContainerRole::Runner,
        format!("container:{dind_id}"),
        platform,
        context,
    )?;
    own(&mut runner, labels, None)?;
    runner
        .config
        .env
        .get_or_insert_default()
        .push(CRATES_MIRROR_OFF.to_owned());
    Ok(runner)
}

fn own(
    create: &mut PairCreate,
    labels: HashMap<String, String>,
    network: Option<&str>,
) -> Result<(), HostError> {
    create.config.labels = Some(labels);
    if let Some(network) = network {
        let host = create
            .config
            .host_config
            .as_mut()
            .ok_or(HostError::Identity)?;
        host.network_mode = Some(network.to_owned());
    }
    Ok(())
}

async fn create_volume(
    docker: &Docker,
    name: String,
    role: WorkerVolumeRole,
    labels: HashMap<String, String>,
    sizes: &Sizes,
) -> Result<(), HostError> {
    let driver_opts = storage::driver_opts(storage_role(role), sizes)?;
    docker_deadline(docker.create_volume(VolumeCreateRequest {
        name: Some(name),
        driver: Some("local".to_owned()),
        driver_opts,
        labels: Some(labels),
        ..Default::default()
    }))
    .await?
    .map_err(|_| HostError::Docker)?;
    Ok(())
}

async fn prefill_metadata(
    docker: &Docker,
    request: &LocalPairRequest,
    validated: &Validated,
) -> Result<(), HostError> {
    let volume = validated
        .names
        .volume_name(WorkerVolumeRole::RunnerMetadata);
    upload_to_metadata(
        docker,
        request,
        &validated.pair,
        &volume,
        MetadataUpload::Initialize(layout::metadata_archive()?),
    )
    .await
}

async fn create(docker: &Docker, create: PairCreate) -> Result<String, HostError> {
    let created = docker_deadline(docker.create_container(Some(create.options), create.config))
        .await?
        .map_err(|_| HostError::Docker)?;
    Ok(created.id)
}

async fn start(docker: &Docker, id: &str) -> Result<(), HostError> {
    docker_deadline(docker.start_container(id, None::<StartContainerOptions>))
        .await?
        .map_err(|_| HostError::Docker)?;
    let mut exited = docker.wait_container(
        id,
        Some(WaitContainerOptions {
            condition: "not-running".to_owned(),
        }),
    );
    if tokio::time::timeout(START_GRACE, exited.next())
        .await
        .is_ok()
    {
        return Err(HostError::Ownership);
    }
    let inspected = docker_deadline(docker.inspect_container(id, None::<InspectContainerOptions>))
        .await?
        .map_err(|_| HostError::Docker)?;
    match inspected.state.and_then(|state| state.running) {
        Some(true) => Ok(()),
        _ => Err(HostError::Ownership),
    }
}

async fn upload(docker: &Docker, id: &str, path: &str, archive: Vec<u8>) -> Result<(), HostError> {
    let options = UploadToContainerOptions {
        path: path.to_owned(),
        ..Default::default()
    };
    docker_deadline(docker.upload_to_container(
        id,
        Some(options),
        bollard::body_full(archive.into()),
    ))
    .await?
    .map_err(|_| HostError::Docker)
}

pub(super) fn owned_labels(
    request: &LocalPairRequest,
    pair: &str,
    role: &str,
) -> HashMap<String, String> {
    HashMap::from([
        (ENVIRONMENT_LABEL.to_owned(), request.environment.clone()),
        (PAIR_LABEL.to_owned(), pair.to_owned()),
        (ROLE_LABEL.to_owned(), role.to_owned()),
        (SIDE_LABEL.to_owned(), request.side.clone()),
        (AGENT_LABEL.to_owned(), request.agent.clone()),
    ])
}

const fn storage_role(role: WorkerVolumeRole) -> StorageRole {
    match role {
        WorkerVolumeRole::Socket => StorageRole::SharedRun,
        WorkerVolumeRole::Home => StorageRole::RunnerHome,
        WorkerVolumeRole::Work => StorageRole::RunnerWork,
        WorkerVolumeRole::DindData => StorageRole::DindData,
        WorkerVolumeRole::RunnerMetadata => StorageRole::EngineMetadata,
    }
}

const fn volume_role(role: WorkerVolumeRole) -> &'static str {
    storage_role(role).as_str()
}

#[cfg(test)]
#[path = "pair_tests.rs"]
mod tests;
