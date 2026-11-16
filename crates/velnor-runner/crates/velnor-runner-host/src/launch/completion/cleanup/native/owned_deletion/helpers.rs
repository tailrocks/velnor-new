// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use bollard::{
    Docker,
    errors::Error as DockerError,
    models::ContainerInspectResponse,
    query_parameters::{ListContainersOptions, StopContainerOptions},
};

use crate::{
    error::HostError,
    journal::{CleanupClaim, CompletedLaunch, StorageCleanupIdentity},
};

use super::super::super::super::{Context, EffectBudget, Failure, MAX_HELPERS_PER_VOLUME};
use super::{inspect_container, remove_container_and_check, request_effect, within};

const HELPER_ROLES: [&str; 3] = [
    "resource-probe",
    "execution-publication",
    "storage-initializer",
];

pub(super) fn docker_error(phase: &str, object: &str, error: &DockerError) -> HostError {
    eprintln!("completion cleanup docker error phase={phase} object={object} error={error}");
    HostError::Docker
}

pub(super) async fn remove_volume_helpers(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    identity: &StorageCleanupIdentity,
    volume: &str,
) -> Result<(), Failure> {
    let users = request_effect(
        context,
        launch,
        claim,
        budget,
        "volume helper list",
        identity,
        || async { list_volume_users(context.docker, volume).await },
    )
    .await?;
    if u64::try_from(users.len()).map_or(true, |count| count > MAX_HELPERS_PER_VOLUME) {
        return Err(Failure::not_proven("volume helper bound"));
    }
    for id in users {
        let inspected = request_effect(
            context,
            launch,
            claim,
            budget,
            "volume helper inspect",
            identity,
            || async { inspect_container(context.docker, &id).await },
        )
        .await?;
        let Some(inspected) = inspected else {
            continue;
        };
        if !is_helper_of(&inspected, &id, volume) {
            return Err(Failure::not_proven("volume helper ownership"));
        }
        if inspected.state.as_ref().and_then(|state| state.running) == Some(true) {
            request_effect(
                context,
                launch,
                claim,
                budget,
                "volume helper stop",
                identity,
                || async { stop_helper(context, &id).await },
            )
            .await?;
        }
        remove_container_and_check(context, launch, claim, budget, &id, identity).await?;
    }
    Ok(())
}

async fn list_volume_users(docker: &Docker, volume: &str) -> Result<Vec<String>, HostError> {
    let filters = HashMap::from([("volume".to_owned(), vec![volume.to_owned()])]);
    let listed = within(docker.list_containers(Some(ListContainersOptions {
        all: true,
        filters: Some(filters),
        ..Default::default()
    })))
    .await?
    .map_err(|error| docker_error("volume helper list", volume, &error))?;
    Ok(listed
        .into_iter()
        .filter_map(|container| container.id)
        .collect())
}

async fn stop_helper(context: &Context<'_>, id: &str) -> Result<(), HostError> {
    within(context.docker.stop_container(
        id,
        Some(StopContainerOptions {
            t: Some(10),
            signal: None,
        }),
    ))
    .await?
    .map_err(|error| docker_error("volume helper stop", id, &error))
}

fn is_helper_of(container: &ContainerInspectResponse, id: &str, volume: &str) -> bool {
    let role = container
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
        .and_then(|labels| labels.get("velnor.role"));
    container.id.as_deref() == Some(id)
        && role.is_some_and(|role| HELPER_ROLES.contains(&role.as_str()))
        && container
            .mounts
            .iter()
            .flatten()
            .any(|mount| mount.name.as_deref() == Some(volume))
}
