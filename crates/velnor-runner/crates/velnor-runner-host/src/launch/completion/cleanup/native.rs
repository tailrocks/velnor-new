// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
use docker::{remove_container, remove_network, remove_owned_volumes, within};
pub(in crate::launch) use docker::{
    remove_owned_resources_for_jit_conflict, remove_owned_resources_for_retirement,
};

fn runner_cleanup_identity(
    launch: &CompletedLaunch,
    selected: &crate::launch::SelectedEngineContext,
    reservation_id: i64,
) -> Result<(String, crate::journal::StorageCleanupIdentity), crate::error::HostError> {
    let runner_id = launch
        .intent
        .docker_id
        .as_deref()
        .ok_or(crate::error::HostError::Identity)?;
    let dind_id = launch
        .intent
        .dind_id
        .as_deref()
        .ok_or(crate::error::HostError::Identity)?;
    let private_volume = launch
        .intent
        .worker_volume
        .as_deref()
        .ok_or(crate::error::HostError::Identity)?;
    let names = WorkerIdentityNames::from_private_volume(private_volume)?;
    let runner_name = names.container_name(WorkerContainerRole::Runner);
    let home_volume = names.volume_name(crate::worker::WorkerVolumeRole::Home);
    Ok((
        runner_name,
        crate::journal::StorageCleanupIdentity::new_runner_cleanup(
            reservation_id,
            launch.intent.id,
            selected.engine_id().to_owned(),
            selected.docker_root_digest().to_owned(),
            runner_id.to_owned(),
            dind_id.to_owned(),
            private_volume.to_owned(),
            home_volume,
        ),
    ))
}

pub async fn remove_completed_runner_container(
    docker: &bollard::Docker,
    journal: &crate::journal::Journal,
    binding: &crate::config::RepositoryConnection,
    state_dir: &std::path::Path,
    reservation_id: i64,
) -> Result<(), crate::error::HostError> {
    let scope = journal.existing_connection_scope(binding).await?;
    let engine = scope
        .engine_context()
        .await?
        .ok_or(crate::error::HostError::Ownership)?;
    let info = within(docker.info())
        .await?
        .map_err(|_| crate::error::HostError::Docker)?;
    let selected = crate::launch::SelectedEngineContext::capture(&engine, &info)?;
    let launch = scope
        .due_completed_launches(super::completion_now()?, 1)
        .await?
        .into_iter()
        .next()
        .ok_or(crate::error::HostError::Journal)?;
    let claim = scope
        .claim_completion_cleanup(launch.intent.id, super::CLAIM_LEASE_SECONDS)
        .await?
        .ok_or(crate::error::HostError::Journal)?;
    let (runner_name, identity) = runner_cleanup_identity(&launch, &selected, reservation_id)?;
    let completion_journal =
        crate::launch::completion::journal_ref::CompletionJournal::scoped(scope);
    let diagnostics = crate::launch::completion::diagnostics::DiagnosticsRoot::open_for_journal(
        state_dir,
        completion_journal.completion_state_identity(),
        completion_journal.connection_id().as_str(),
    )?;
    let mut transport = crate::https::HttpsTransport::new("https://api.github.com")?;
    let admin = crate::listen::Secret::new("token");
    let stopping = std::sync::atomic::AtomicBool::new(false);
    let mut budget = EffectBudget::new(&stopping);
    let context = Context {
        journal: &completion_journal,
        docker,
        transport: &mut transport,
        admin: &admin,
        diagnostics: &diagnostics,
        native_cleanup: None,
        selected_engine: Some(&selected),
    };
    let target = docker::ContainerDeletion {
        id: identity.runner_container_id(),
        name: &runner_name,
        role: "runner",
        identity: &identity,
    };
    let outcome = Box::pin(remove_container(
        &context,
        &launch,
        claim,
        &mut budget,
        &target,
    ))
    .await;
    completion_journal
        .retry_completion_cleanup(launch.intent.id, claim.generation, 0)
        .await?;
    outcome.map_err(|failure| failure.error)
}
