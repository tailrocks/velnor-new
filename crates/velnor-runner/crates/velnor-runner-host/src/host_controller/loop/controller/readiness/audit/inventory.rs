// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;
use serde::Serialize;

use crate::{
    config::{HostConfig, RepositoryConnection},
    error::HostError,
    journal::{
        IntentState, Journal, PhysicalRunnerJitState, PhysicalRunnerOwnership,
        PhysicalRunnerReservation, ReservationState, StorageCleanupIdentity,
    },
    launch::SelectedEngineContext,
    worker::{WorkerContainerRole, WorkerIdentityNames},
};

use super::{
    ExpectedContainer, OwnedPair, RemoteRunnerAudit, audit_pair_containers, inspect_owned_volume,
};

#[path = "inventory/settling.rs"]
mod settling;
use settling::Inventory;
pub(in crate::host_controller::loop_driver::controller) use settling::SettlingAges;

pub(in crate::host_controller::loop_driver::controller) async fn audit_inventory_with_remote(
    docker: &Docker,
    journal: &Journal,
    config: &HostConfig,
    selected: &SelectedEngineContext,
    polls: &[crate::controller_ipc::SessionPollReceipt],
    dependencies: &super::super::RefreshDependencies<'_>,
) -> Result<Option<String>, HostError> {
    let mut inventory = Inventory::default();
    for binding in config.connections() {
        let poll = polls
            .iter()
            .find(|poll| poll.connection_id() == binding.connection_id.as_str())
            .ok_or(HostError::Identity)?;
        audit_connection(
            docker,
            journal,
            binding,
            selected,
            poll,
            dependencies.remote_audit,
            &mut inventory,
        )
        .await?;
    }
    inventory.finish(dependencies.settling, std::time::Instant::now())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PairStage {
    Running,
    Settling,
}

fn pair_stage(
    ownership: &PhysicalRunnerOwnership,
    state: PhysicalRunnerJitState,
) -> Result<PairStage, HostError> {
    let resolved = match state {
        PhysicalRunnerJitState::Ready | PhysicalRunnerJitState::Started => PairStage::Running,
        PhysicalRunnerJitState::Absent
        | PhysicalRunnerJitState::Claimed
        | PhysicalRunnerJitState::Bound
        | PhysicalRunnerJitState::StartRequested
        | PhysicalRunnerJitState::Completed
        | PhysicalRunnerJitState::Cleaned => PairStage::Settling,
        PhysicalRunnerJitState::Uncertain
        | PhysicalRunnerJitState::StartUncertain
        | PhysicalRunnerJitState::Unknown => return Err(HostError::Ownership),
    };
    if resolved == PairStage::Running && *ownership == PhysicalRunnerOwnership::Unknown {
        return Err(HostError::Ownership);
    }
    Ok(resolved)
}

async fn audit_connection(
    docker: &Docker,
    journal: &Journal,
    binding: &RepositoryConnection,
    selected: &SelectedEngineContext,
    poll: &crate::controller_ipc::SessionPollReceipt,
    remote_audit: &RemoteRunnerAudit,
    inventory: &mut Inventory,
) -> Result<(), HostError> {
    let scope = journal.existing_connection_scope(binding).await?;
    validate_session_scope(&scope, binding, poll).await?;
    for row in scope.physical_runner_reservations().await? {
        if pair_stage(&row.ownership, row.jit_state)? == PairStage::Settling {
            inventory.settle(binding.connection_id.as_str(), row.reservation_id);
            continue;
        }
        let PhysicalRunnerOwnership::Known(assignment) = &row.ownership else {
            return Err(HostError::Ownership);
        };
        if assignment.scale_set_id() != poll.scale_set_id() {
            return Err(HostError::Ownership);
        }
        if !scope
            .readiness_session_matches(
                poll.session_intent_id(),
                poll.scale_set_id(),
                poll.session_id(),
            )
            .await?
        {
            return Err(HostError::Ownership);
        }
        if row.reservation.state != ReservationState::Reserved
            || row.intent_state != Some(IntentState::Pending)
            || row.completion_cleanup_pending
            || row.worker_cleanup_proven()
            || row.remote_cleanup_proven()
            || row.local_cleanup_proven()
        {
            return Err(HostError::Ownership);
        }
        let identity = scope
            .storage_cleanup_identity(row.reservation_id)
            .await?
            .ok_or(HostError::Ownership)?;
        audit_started_pair(docker, binding, selected, &row, &identity).await?;
        let remote_present = remote_audit(binding, row.runner.as_ref(), poll.scale_set_id())?;
        inventory.admit(
            remote_present,
            InventoryEntry::started(
                binding.connection_id.as_str(),
                row.reservation_id,
                assignment,
                &row,
                identity.runner_container_id(),
                identity.dind_container_id(),
            ),
        );
    }
    Ok(())
}

async fn validate_session_scope(
    scope: &crate::journal::ConnectionJournal,
    binding: &RepositoryConnection,
    poll: &crate::controller_ipc::SessionPollReceipt,
) -> Result<(), HostError> {
    if poll.connection_id() == binding.connection_id.as_str()
        && scope
            .readiness_session_matches(
                poll.session_intent_id(),
                poll.scale_set_id(),
                poll.session_id(),
            )
            .await?
    {
        Ok(())
    } else {
        Err(HostError::Ownership)
    }
}

async fn audit_started_pair(
    docker: &Docker,
    binding: &RepositoryConnection,
    selected: &SelectedEngineContext,
    row: &PhysicalRunnerReservation,
    identity: &StorageCleanupIdentity,
) -> Result<(), HostError> {
    if identity.engine_id() != selected.engine_id()
        || identity.docker_root_digest() != selected.docker_root_digest()
        || row.runner_container_id.as_deref() != Some(identity.runner_container_id())
        || row.dind_container_id.as_deref() != Some(identity.dind_container_id())
    {
        return Err(HostError::Ownership);
    }
    let names = WorkerIdentityNames::from_private_volume(identity.private_volume())?;
    audit_pair_containers(
        docker,
        &OwnedPair {
            connection_id: binding.connection_id.as_str(),
            reservation_id: identity.reservation_id(),
            operation_id: identity.operation_id(),
        },
        &[
            ExpectedContainer {
                id: identity.runner_container_id(),
                name: names.container_name(WorkerContainerRole::Runner),
                role: "runner",
                image_id: identity.runner_image_id(),
            },
            ExpectedContainer {
                id: identity.dind_container_id(),
                name: names.container_name(WorkerContainerRole::Dind),
                role: "dind",
                image_id: identity.dind_image_id(),
            },
        ],
    )
    .await?;
    for (role, name) in identity.volumes() {
        inspect_owned_volume(
            docker,
            name,
            *role,
            identity,
            binding.connection_id.as_str(),
        )
        .await?;
    }
    Ok(())
}

#[derive(Serialize)]
struct InventoryEntry {
    connection_id: String,
    reservation_id: i64,
    physical_assignment_key: String,
    witness_request_id: Option<i64>,
    acquired_request_key: Option<String>,
    runner_request_id: Option<i64>,
    started_request_id: Option<i64>,
    completed_request_id: Option<i64>,
    runner_id: Option<String>,
    dind_id: Option<String>,
}

impl InventoryEntry {
    fn started(
        connection_id: &str,
        reservation_id: i64,
        assignment: &crate::journal::AssignmentIdentity,
        row: &PhysicalRunnerReservation,
        runner_id: &str,
        dind_id: &str,
    ) -> Self {
        Self {
            connection_id: connection_id.to_owned(),
            reservation_id,
            physical_assignment_key: assignment.storage_key(),
            witness_request_id: row.witness_request_id,
            acquired_request_key: row
                .acquired_request
                .as_ref()
                .map(crate::journal::AssignmentIdentity::storage_key),
            runner_request_id: row.runner_request_id,
            started_request_id: row.started_request_id,
            completed_request_id: row.completed_request_id,
            runner_id: Some(runner_id.to_owned()),
            dind_id: Some(dind_id.to_owned()),
        }
    }
}

#[cfg(test)]
#[path = "inventory/tests.rs"]
mod tests;
