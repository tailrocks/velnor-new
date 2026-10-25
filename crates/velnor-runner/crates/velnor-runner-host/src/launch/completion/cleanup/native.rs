//! This module does cleanup for a completed scoped runner and its capacity reservation.

#[cfg(test)]
#[path = "native/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "native/worker_health_tests.rs"]
mod worker_health_tests;

#[cfg(test)]
#[path = "native/diagnostics_tests.rs"]
mod diagnostics_tests;

use crate::{
    journal::{CleanupClaim, CompletedLaunch, MetadataSettlementResult},
    launch::bootstrap::PendingStorageCleanupWitness,
    worker::{WorkerContainerRole, WorkerIdentityNames},
};

use super::{Context, EffectBudget, Failure};

pub(super) async fn cleanup(
    context: &mut Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
) -> Result<(), Failure> {
    let scope = context.journal.connection();
    let runtime = context
        .native_cleanup
        .ok_or_else(|| Failure::not_proven("native cleanup capability"))?;
    let reservation = context
        .journal
        .native_completion_reservation(launch)
        .await
        .map_err(|error| Failure::request("native reservation lookup", error))?
        .ok_or_else(|| Failure::not_proven("native reservation identity"))?;
    if reservation.intent_id() != launch.intent.id
        || reservation.runner_container_id()
            != launch.intent.docker_id.as_deref().unwrap_or_default()
        || reservation.dind_container_id() != launch.intent.dind_id.as_deref().unwrap_or_default()
    {
        return Err(Failure::not_proven("native reservation binding"));
    }
    let phases = CleanupPhases::from_reservation(&reservation);
    if phases.remote {
        super::runner::reconcile(context, launch, claim, budget).await?;
        scope
            .record_remote_cleanup(reservation.reservation_id())
            .await
            .map_err(|error| Failure::request("remote cleanup record", error))?;
    }

    if phases.storage {
        let pending = runtime
            .prepare_storage_cleanup(scope, reservation.reservation_id())
            .await
            .map_err(|error| Failure::request("storage cleanup preflight", error))?;
        validate_pending(&pending, &reservation)?;
        Box::pin(remove_owned_containers(
            context, launch, claim, budget, &pending,
        ))
        .await?;
        remove_owned_volumes(context, launch, claim, budget, &pending).await?;

        let proof = runtime
            .verify_storage_cleanup(scope, pending)
            .await
            .map_err(|error| Failure::request("storage cleanup proof", error))?;
        scope
            .record_storage_cleanup(proof)
            .await
            .map_err(|error| Failure::request("storage cleanup record", error))?;
    }
    if phases.network {
        remove_network(
            context,
            runtime,
            launch,
            claim,
            budget,
            scope,
            reservation.reservation_id(),
        )
        .await?;
    }
    if phases.local {
        scope
            .record_local_cleanup(reservation.reservation_id())
            .await
            .map_err(|error| Failure::request("local cleanup record", error))?;
    }
    let settlement = runtime
        .settle_storage_metadata_after_cleanup(scope, reservation.reservation_id())
        .await
        .map_err(|error| Failure::request("metadata settlement", error))?;
    finish_after_settlement(settlement)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the four fields independently record whether remote, storage, network, and local cleanup remain unproven"
)]
struct CleanupPhases {
    remote: bool,
    storage: bool,
    network: bool,
    local: bool,
}

impl CleanupPhases {
    fn from_reservation(reservation: &crate::journal::NativeCompletionReservation) -> Self {
        Self::from_proofs(
            reservation.remote_cleanup_proven(),
            reservation.storage_cleanup_proven(),
            reservation.network_cleanup_proven(),
            reservation.local_cleanup_proven(),
        )
    }

    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "resume maps each persisted proof independently to the cleanup phase that remains unproven"
    )]
    fn from_proofs(remote: bool, storage: bool, network: bool, local: bool) -> Self {
        Self {
            remote: !remote,
            storage: !storage,
            network: !network,
            local: !local,
        }
    }
}

fn finish_after_settlement(settlement: MetadataSettlementResult) -> Result<(), Failure> {
    match settlement {
        MetadataSettlementResult::Settled => Ok(()),
        MetadataSettlementResult::HeldAtCapacity => {
            Err(Failure::not_proven("metadata capacity hold"))
        }
    }
}

fn validate_pending(
    pending: &PendingStorageCleanupWitness,
    reservation: &crate::journal::NativeCompletionReservation,
) -> Result<(), Failure> {
    let identity = pending.identity();
    if identity.reservation_id() != reservation.reservation_id()
        || identity.operation_id() != reservation.intent_id()
        || identity.engine_id() != reservation.engine_id()
        || identity.docker_root_digest() != reservation.docker_root_digest()
        || identity.runner_container_id() != reservation.runner_container_id()
        || identity.dind_container_id() != reservation.dind_container_id()
        || identity.instance_binding_digest() != reservation.instance_binding_digest()
    {
        return Err(Failure::not_proven("cleanup witness identity"));
    }
    Ok(())
}

async fn remove_owned_containers(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    pending: &PendingStorageCleanupWitness,
) -> Result<(), Failure> {
    let identity = pending.identity();
    let names = WorkerIdentityNames::from_private_volume(identity.private_volume())
        .map_err(|error| Failure::request("worker name derivation", error))?;
    let expected: [(&str, String, &str); 2] = [
        (
            identity.runner_container_id(),
            names.container_name(WorkerContainerRole::Runner),
            "runner",
        ),
        (
            identity.dind_container_id(),
            names.container_name(WorkerContainerRole::Dind),
            "dind",
        ),
    ];
    for (container_id, name, role) in expected {
        let target = docker::ContainerDeletion {
            id: container_id,
            name: &name,
            role,
            identity,
        };
        Box::pin(remove_container(context, launch, claim, budget, &target)).await?;
    }
    Ok(())
}

#[path = "native/docker.rs"]
mod docker;
use docker::{remove_container, remove_network, remove_owned_volumes};
pub(in crate::launch) use docker::{
    remove_owned_resources_for_jit_conflict, remove_owned_resources_for_retirement,
};
