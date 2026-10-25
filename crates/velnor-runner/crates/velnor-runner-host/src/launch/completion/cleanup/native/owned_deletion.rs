//! The durable worker identity controls Docker cleanup requests.

use bollard::{
    Docker,
    errors::Error as DockerError,
    models::{ContainerInspectResponse, Volume},
    query_parameters::{RemoveContainerOptions, StopContainerOptions},
};

use crate::{
    docker_client::docker_deadline_after,
    error::HostError,
    journal::{CleanupClaim, CompletedLaunch, EngineContext, StorageRole},
    launch::bootstrap::{NativeCleanupRuntime, PendingStorageCleanupWitness},
};

use crate::launch::completion::diagnostics;

use super::super::super::{Context, EffectBudget, Failure, run_effect};

#[path = "owned_deletion/identity.rs"]
mod identity;
#[path = "owned_deletion/retirement.rs"]
mod retirement;
#[path = "owned_deletion/runner_logs.rs"]
mod runner_logs;
pub(in crate::launch) use retirement::{
    remove_owned_resources_for_jit_conflict, remove_owned_resources_for_retirement,
};

const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub(in crate::launch::completion::cleanup::native) struct ContainerDeletion<'a> {
    pub(in crate::launch::completion::cleanup::native) id: &'a str,
    pub(in crate::launch::completion::cleanup::native) name: &'a str,
    pub(in crate::launch::completion::cleanup::native) role: &'static str,
    pub(in crate::launch::completion::cleanup::native) identity:
        &'a crate::journal::StorageCleanupIdentity,
}

pub(in crate::launch::completion::cleanup::native) async fn remove_container(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    target: &ContainerDeletion<'_>,
) -> Result<(), Failure> {
    let ContainerDeletion {
        id,
        name,
        role,
        identity,
    } = target;
    let inspected = request_effect(
        context,
        launch,
        claim,
        budget,
        "container inspect",
        identity,
        || async { inspect_container(context.docker, id).await },
    )
    .await?;
    let Some(inspected) = inspected else {
        if *role == "runner" {
            runner_logs::capture_if_needed(context, launch, claim, budget, target).await?;
        }
        return Ok(());
    };
    validate_container(&inspected, id, name, role, identity, context)?;
    let running = inspected.state.as_ref().and_then(|state| state.running) == Some(true);
    let captured = diagnostics::capture_native(
        context,
        diagnostics::NativeCaptureRequest {
            root: context.diagnostics,
            launch,
            claim,
            budget,
            role,
            expected_name: name,
            container_id: id,
            identity,
        },
        identity::validate_container,
    )
    .await;
    Box::pin(diagnostics::delete_after_capture(captured, async {
        if *role == "runner" {
            runner_logs::stop_and_confirm(context, launch, claim, budget, target, running).await?;
            runner_logs::capture_if_needed(context, launch, claim, budget, target).await?;
        } else if running {
            request_effect(
                context,
                launch,
                claim,
                budget,
                "container stop",
                identity,
                || async {
                    within(context.docker.stop_container(
                        id,
                        Some(StopContainerOptions {
                            t: Some(10),
                            signal: None,
                        }),
                    ))
                    .await?
                    .map_err(|_| HostError::Docker)
                },
            )
            .await?;
        }
        remove_container_and_check(context, launch, claim, budget, id, identity).await
    }))
    .await
}

async fn remove_container_and_check(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    id: &str,
    identity: &crate::journal::StorageCleanupIdentity,
) -> Result<(), Failure> {
    request_effect(
        context,
        launch,
        claim,
        budget,
        "container remove",
        identity,
        || async {
            within(context.docker.remove_container(
                id,
                Some(RemoveContainerOptions {
                    force: false,
                    v: false,
                    link: false,
                }),
            ))
            .await?
            .map_err(|_| HostError::Docker)
        },
    )
    .await?;
    let remaining = request_effect(
        context,
        launch,
        claim,
        budget,
        "container absence",
        identity,
        || async { inspect_container(context.docker, id).await },
    )
    .await?;
    if remaining.is_some() {
        return Err(Failure::not_proven("container absence"));
    }
    Ok(())
}

pub(super) fn validate_container(
    container: &ContainerInspectResponse,
    id: &str,
    name: &str,
    role: &str,
    identity: &crate::journal::StorageCleanupIdentity,
    context: &Context<'_>,
) -> Result<(), Failure> {
    let connection_id = context.journal.connection().connection_id().as_str();
    identity::validate_container(container, id, name, role, identity, connection_id)
        .map_err(|_| Failure::not_proven("container ownership"))
}

pub(in crate::launch::completion::cleanup::native) async fn remove_owned_volumes(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    pending: &PendingStorageCleanupWitness,
) -> Result<(), Failure> {
    let identity = pending.identity();
    for (role, name) in identity.volumes() {
        let volume = request_effect(
            context,
            launch,
            claim,
            budget,
            "volume inspect",
            identity,
            || async { inspect_volume(context.docker, name).await },
        )
        .await?;
        let Some(volume) = volume else {
            continue;
        };
        validate_volume(&volume, name, *role, identity, context)?;
        request_effect(
            context,
            launch,
            claim,
            budget,
            "volume remove",
            identity,
            || async {
                within(
                    context.docker.remove_volume(
                        name,
                        None::<bollard::query_parameters::RemoveVolumeOptions>,
                    ),
                )
                .await?
                .map_err(|_| HostError::Docker)
            },
        )
        .await?;
        if request_effect(
            context,
            launch,
            claim,
            budget,
            "volume absence",
            identity,
            || async { inspect_volume(context.docker, name).await },
        )
        .await?
        .is_some()
        {
            return Err(Failure::not_proven("volume absence"));
        }
    }
    Ok(())
}

fn validate_volume(
    volume: &Volume,
    name: &str,
    role: StorageRole,
    identity: &crate::journal::StorageCleanupIdentity,
    context: &Context<'_>,
) -> Result<(), Failure> {
    let connection_id = context.journal.connection().connection_id().as_str();
    identity::validate_volume(volume, name, role, identity, connection_id)
        .map_err(|_| Failure::not_proven("volume ownership"))
}

pub(in crate::launch::completion::cleanup::native) async fn remove_network(
    context: &Context<'_>,
    runtime: &NativeCleanupRuntime,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
) -> Result<(), Failure> {
    for _ in 1..7 {
        budget.reserve("network remove")?;
    }
    run_effect(
        context.journal,
        budget,
        launch.intent.id,
        claim,
        "network remove",
        || async { runtime.remove_owned_network(scope, reservation_id).await },
    )
    .await
}

async fn verify_runtime(
    context: &Context<'_>,
    identity: &crate::journal::StorageCleanupIdentity,
) -> Result<(), HostError> {
    let selected = context.selected_engine.ok_or(HostError::Ownership)?;
    let durable = EngineContext {
        engine_id: selected.engine_id().to_owned(),
        endpoint: selected.endpoint().to_owned(),
        platform: selected.platform().to_owned(),
    };
    let info = within(context.docker.info())
        .await?
        .map_err(|_| HostError::Docker)?;
    let current = crate::launch::SelectedEngineContext::capture(&durable, &info)?;
    if current.matches_selection(&durable)
        && current.engine_id() == identity.engine_id()
        && current.docker_root_digest() == identity.docker_root_digest()
    {
        Ok(())
    } else {
        Err(HostError::Ownership)
    }
}

pub(super) async fn request_effect<T, F, Fut>(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    phase: &'static str,
    identity: &crate::journal::StorageCleanupIdentity,
    effect: F,
) -> Result<T, Failure>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, HostError>>,
{
    run_effect(
        context.journal,
        budget,
        launch.intent.id,
        claim,
        "engine identity",
        || async { verify_runtime(context, identity).await },
    )
    .await?;
    run_effect(
        context.journal,
        budget,
        launch.intent.id,
        claim,
        phase,
        effect,
    )
    .await
}

pub(super) async fn inspect_container(
    docker: &Docker,
    id: &str,
) -> Result<Option<ContainerInspectResponse>, HostError> {
    match within(docker.inspect_container(id, None)).await? {
        Ok(response) => Ok(Some(response)),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

async fn inspect_volume(docker: &Docker, name: &str) -> Result<Option<Volume>, HostError> {
    match within(docker.inspect_volume(name)).await? {
        Ok(volume) => Ok(Some(volume)),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

pub(super) async fn within<F: std::future::Future>(future: F) -> Result<F::Output, HostError> {
    docker_deadline_after(future, REQUEST_TIMEOUT).await
}
