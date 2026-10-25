use std::future::Future;

use bollard::{
    Docker,
    query_parameters::{RemoveContainerOptions, StopContainerOptions},
};

use crate::{
    error::HostError,
    journal::{
        ConnectionJournal, JitConflictRecoveryClaim, PostJitRetirementClaim, StorageCleanupIdentity,
    },
    launch::{SelectedEngineContext, bootstrap::PendingStorageCleanupWitness},
    worker::{WorkerContainerRole, WorkerIdentityNames},
};

use super::{ContainerDeletion, identity, inspect_container, inspect_volume, within};

pub(in crate::launch) async fn remove_owned_resources_for_retirement(
    docker: &Docker,
    scope: &ConnectionJournal,
    claim: &PostJitRetirementClaim,
    engine: &SelectedEngineContext,
    pending: &PendingStorageCleanupWitness,
) -> Result<(), HostError> {
    let identity = pending.identity();
    let context = DeletionContext {
        docker,
        scope,
        authority: DeletionAuthority::Retirement(claim),
        engine,
        identity,
    };
    context.authorize().await?;
    Box::pin(remove_containers(&context)).await?;
    Box::pin(remove_volumes(&context)).await
}

pub(in crate::launch) async fn remove_owned_resources_for_jit_conflict(
    docker: &Docker,
    scope: &ConnectionJournal,
    claim: &JitConflictRecoveryClaim,
    engine: &SelectedEngineContext,
    pending: &PendingStorageCleanupWitness,
) -> Result<(), HostError> {
    let identity = pending.identity();
    let context = DeletionContext {
        docker,
        scope,
        authority: DeletionAuthority::Conflict(claim),
        engine,
        identity,
    };
    context.authorize().await?;
    Box::pin(remove_containers(&context)).await?;
    Box::pin(remove_volumes(&context)).await
}

enum DeletionAuthority<'a> {
    Retirement(&'a PostJitRetirementClaim),
    Conflict(&'a JitConflictRecoveryClaim),
}

struct DeletionContext<'a> {
    docker: &'a Docker,
    scope: &'a ConnectionJournal,
    authority: DeletionAuthority<'a>,
    engine: &'a SelectedEngineContext,
    identity: &'a StorageCleanupIdentity,
}

impl DeletionContext<'_> {
    async fn authorize(&self) -> Result<(), HostError> {
        if self.engine.engine_id() != self.identity.engine_id()
            || self.engine.docker_root_digest() != self.identity.docker_root_digest()
        {
            return Err(HostError::Ownership);
        }
        self.validate_authority().await?;
        verify_engine(self.docker, self.engine, self.identity).await?;
        self.validate_authority().await
    }

    async fn validate_authority(&self) -> Result<(), HostError> {
        match &self.authority {
            DeletionAuthority::Retirement(claim) => {
                self.scope
                    .validate_retirement_claim_for_cleanup(claim, self.identity)
                    .await
            }
            DeletionAuthority::Conflict(claim) => {
                if claim.reservation_id() != self.identity.reservation_id()
                    || claim.intent_id() != self.identity.operation_id()
                    || claim.engine_id() != self.identity.engine_id()
                    || claim.docker_root_digest() != self.identity.docker_root_digest()
                {
                    return Err(HostError::Identity);
                }
                self.scope.validate_jit_conflict_claim(claim).await
            }
        }
    }

    async fn operation<T, F>(&self, effect: F) -> Result<T, HostError>
    where
        F: Future<Output = Result<T, HostError>>,
    {
        self.authorize().await?;
        let result = effect.await;
        self.authorize().await?;
        result
    }
}

async fn remove_containers(context: &DeletionContext<'_>) -> Result<(), HostError> {
    let names = WorkerIdentityNames::from_private_volume(context.identity.private_volume())?;
    let runner_name = names.container_name(WorkerContainerRole::Runner);
    let dind_name = names.container_name(WorkerContainerRole::Dind);
    let targets = [
        ContainerDeletion {
            id: context.identity.runner_container_id(),
            name: &runner_name,
            role: "runner",
            identity: context.identity,
        },
        ContainerDeletion {
            id: context.identity.dind_container_id(),
            name: &dind_name,
            role: "dind",
            identity: context.identity,
        },
    ];
    for target in &targets {
        Box::pin(remove_container(context, target)).await?;
    }
    Ok(())
}

async fn remove_container(
    context: &DeletionContext<'_>,
    target: &ContainerDeletion<'_>,
) -> Result<(), HostError> {
    let inspected =
        Box::pin(context.operation(inspect_container(context.docker, target.id))).await?;
    let Some(inspected) = inspected else {
        return Ok(());
    };
    identity::validate_container(
        &inspected,
        target.id,
        target.name,
        target.role,
        target.identity,
        context.scope.connection_id().as_str(),
    )?;
    if inspected.state.as_ref().and_then(|state| state.running) == Some(true) {
        let stop = context.docker.stop_container(
            target.id,
            Some(StopContainerOptions {
                t: Some(10),
                signal: None,
            }),
        );
        Box::pin(context.operation(async { within(stop).await?.map_err(|_| HostError::Docker) }))
            .await?;
    }
    let remove = context.docker.remove_container(
        target.id,
        Some(RemoveContainerOptions {
            force: false,
            v: false,
            link: false,
        }),
    );
    Box::pin(context.operation(async { within(remove).await?.map_err(|_| HostError::Docker) }))
        .await?;
    if Box::pin(context.operation(inspect_container(context.docker, target.id)))
        .await?
        .is_some()
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

async fn remove_volumes(context: &DeletionContext<'_>) -> Result<(), HostError> {
    for (role, name) in context.identity.volumes() {
        let inspected = Box::pin(context.operation(inspect_volume(context.docker, name))).await?;
        let Some(volume) = inspected else {
            continue;
        };
        identity::validate_volume(
            &volume,
            name,
            *role,
            context.identity,
            context.scope.connection_id().as_str(),
        )?;
        let remove = context
            .docker
            .remove_volume(name, None::<bollard::query_parameters::RemoveVolumeOptions>);
        Box::pin(context.operation(async { within(remove).await?.map_err(|_| HostError::Docker) }))
            .await?;
        if Box::pin(context.operation(inspect_volume(context.docker, name)))
            .await?
            .is_some()
        {
            return Err(HostError::Ownership);
        }
    }
    Ok(())
}

async fn verify_engine(
    docker: &Docker,
    selected: &SelectedEngineContext,
    identity: &StorageCleanupIdentity,
) -> Result<(), HostError> {
    let durable = crate::journal::EngineContext {
        engine_id: selected.engine_id().to_owned(),
        endpoint: selected.endpoint().to_owned(),
        platform: selected.platform().to_owned(),
    };
    let info = within(docker.info())
        .await?
        .map_err(|_| HostError::Docker)?;
    let current = SelectedEngineContext::capture(&durable, &info)?;
    if current.matches_selection(&durable)
        && current.engine_id() == identity.engine_id()
        && current.docker_root_digest() == identity.docker_root_digest()
    {
        Ok(())
    } else {
        Err(HostError::Ownership)
    }
}

#[cfg(test)]
#[path = "retirement_tests.rs"]
mod tests;
