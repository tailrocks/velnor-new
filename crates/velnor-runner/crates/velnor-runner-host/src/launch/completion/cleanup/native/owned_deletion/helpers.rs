// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use bollard::{
    Docker,
    errors::Error as DockerError,
    models::ContainerInspectResponse,
    query_parameters::{ListContainersOptions, StopContainerOptions},
};
use sha2::{Digest, Sha256};

use crate::{
    error::HostError,
    journal::{CleanupClaim, CompletedLaunch, StorageCleanupIdentity},
};

use super::super::super::super::{Context, EffectBudget, Failure, MAX_HELPERS_PER_VOLUME};
use super::{inspect_container, remove_container_and_check, request_effect, within};
use crate::journal::UnboundInitializer;
use crate::launch::bootstrap::{InitializerPlan, plan_initializer_removal};

pub(super) fn docker_error(phase: &str, object: &str, error: &DockerError) -> HostError {
    eprintln!("completion cleanup docker error phase={phase} object={object} error={error}");
    HostError::Docker
}

enum Retirement {
    Publication(crate::launch::resource_probe::OwnedPublication),
    Initializer { id: String, running: bool },
}

pub(super) async fn remove_volume_helpers(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    identity: &StorageCleanupIdentity,
    volume: &str,
) -> Result<(), Failure> {
    let users = list_users(context, launch, claim, budget, identity, volume).await?;
    if u64::try_from(users.len()).map_or(true, |count| count > MAX_HELPERS_PER_VOLUME) {
        return Err(Failure::not_proven("volume helper bound"));
    }
    let runtime = context
        .native_cleanup
        .ok_or_else(|| Failure::not_proven("native cleanup capability"))?;
    let scope = context.journal.connection();
    let initializer = request_effect(
        context,
        launch,
        claim,
        budget,
        "volume helper initializer lookup",
        identity,
        || async {
            scope
                .unretired_storage_initializer(
                    identity.reservation_id(),
                    identity.engine_id(),
                    identity.docker_root_digest(),
                )
                .await
        },
    )
    .await?;
    if !users.is_empty() {
        let publication = request_effect(
            context,
            launch,
            claim,
            budget,
            "volume helper publication lookup",
            identity,
            || async { runtime.owned_publication(scope, identity).await },
        )
        .await?;
        let plan = classify_users(
            context,
            launch,
            claim,
            budget,
            identity,
            volume,
            (users, publication, initializer.clone()),
        )
        .await?;
        for retirement in plan {
            retire(context, launch, claim, budget, identity, retirement).await?;
        }
        let remaining = list_users(context, launch, claim, budget, identity, volume).await?;
        if !remaining.is_empty() {
            return Err(Failure::not_proven("volume helper retirement"));
        }
    }
    match initializer {
        Some(helper) => settle_initializer(context, launch, claim, budget, identity, &helper).await,
        None => Ok(()),
    }
}

async fn settle_initializer(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    identity: &StorageCleanupIdentity,
    helper: &UnboundInitializer,
) -> Result<(), Failure> {
    let by_name = request_effect(
        context,
        launch,
        claim,
        budget,
        "initializer name absence",
        identity,
        || async { inspect_container(context.docker, &helper.name).await },
    )
    .await?;
    let by_id = match helper.id.as_deref() {
        Some(id) => {
            request_effect(
                context,
                launch,
                claim,
                budget,
                "initializer id absence",
                identity,
                || async { inspect_container(context.docker, id).await },
            )
            .await?
        }
        None => None,
    };
    if by_name.is_some() || by_id.is_some() {
        return Ok(());
    }
    let scope = context.journal.connection();
    request_effect(
        context,
        launch,
        claim,
        budget,
        "initializer journal settlement",
        identity,
        || async {
            scope
                .settle_removed_storage_initializer(
                    identity.reservation_id(),
                    identity.engine_id(),
                    identity.docker_root_digest(),
                    helper,
                )
                .await
        },
    )
    .await
}

async fn classify_users(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    identity: &StorageCleanupIdentity,
    volume: &str,
    owned: (
        Vec<String>,
        Option<crate::launch::resource_probe::OwnedPublication>,
        Option<UnboundInitializer>,
    ),
) -> Result<Vec<Retirement>, Failure> {
    let (users, mut publication, initializer) = owned;
    let mut plan = Vec::new();
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
        if !mounts_volume(&inspected, &id, volume) {
            return Err(Failure::not_proven("volume helper ownership"));
        }
        if let Some(owned) = publication
            .take_if(|owned| is_journaled_publication(&inspected, owned, identity.engine_id()))
        {
            plan.push(Retirement::Publication(owned));
        } else if let Some(helper) = initializer.as_ref()
            && let Ok(InitializerPlan::Remove { id: planned }) =
                plan_initializer_removal(helper, Some(&inspected), None)
            && planned == id
        {
            plan.push(Retirement::Initializer {
                id,
                running: inspected.state.as_ref().and_then(|state| state.running) == Some(true),
            });
        } else {
            return Err(Failure::not_proven("volume helper ownership"));
        }
    }
    Ok(plan)
}

async fn retire(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    identity: &StorageCleanupIdentity,
    retirement: Retirement,
) -> Result<(), Failure> {
    let runtime = context
        .native_cleanup
        .ok_or_else(|| Failure::not_proven("native cleanup capability"))?;
    match retirement {
        Retirement::Publication(owned) => {
            request_effect(
                context,
                launch,
                claim,
                budget,
                "volume helper publication retire",
                identity,
                || async { runtime.retire_owned_publication(&owned).await },
            )
            .await?;
            let retired = request_effect(
                context,
                launch,
                claim,
                budget,
                "volume helper publication lease",
                identity,
                || async { runtime.publication_retired(&owned).await },
            )
            .await?;
            if retired {
                Ok(())
            } else {
                Err(Failure::not_proven("volume helper publication lease"))
            }
        }
        Retirement::Initializer { id, running } => {
            if running {
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
            remove_container_and_check(context, launch, claim, budget, &id, identity).await
        }
    }
}

async fn list_users(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    identity: &StorageCleanupIdentity,
    volume: &str,
) -> Result<Vec<String>, Failure> {
    request_effect(
        context,
        launch,
        claim,
        budget,
        "volume helper list",
        identity,
        || async { list_volume_users(context.docker, volume).await },
    )
    .await
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

fn mounts_volume(container: &ContainerInspectResponse, id: &str, volume: &str) -> bool {
    container.id.as_deref() == Some(id)
        && container
            .mounts
            .iter()
            .flatten()
            .any(|mount| mount.name.as_deref() == Some(volume))
}

fn engine_digest(engine_id: &str) -> String {
    Sha256::digest(engine_id.as_bytes())
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 0x0f])
        .filter_map(|nibble| char::from_digit(u32::from(nibble), 16))
        .collect()
}

fn is_journaled_publication(
    container: &ContainerInspectResponse,
    owned: &crate::launch::resource_probe::OwnedPublication,
    engine_id: &str,
) -> bool {
    let labels = container
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref());
    let label = |key: &str| {
        labels
            .and_then(|labels| labels.get(key))
            .map(String::as_str)
    };
    container.name.as_deref() == Some(format!("/{}", owned.operation_name()).as_str())
        && owned
            .container_id()
            .is_none_or(|id| container.id.as_deref() == Some(id))
        && label("velnor.role") == Some("execution-publication")
        && label("velnor.operation") == Some(owned.operation_id())
        && label("velnor.engine") == Some(engine_digest(engine_id).as_str())
}
