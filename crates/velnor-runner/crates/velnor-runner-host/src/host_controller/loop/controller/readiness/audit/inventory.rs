//! Check configured connections and encode the exact physical inventory.

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

use super::{RemoteRunnerAudit, inspect_owned_container, inspect_owned_volume};

pub(in crate::host_controller::loop_driver::controller) async fn audit_inventory_with_remote(
    docker: &Docker,
    journal: &Journal,
    config: &HostConfig,
    selected: &SelectedEngineContext,
    polls: &[crate::controller_ipc::SessionPollReceipt],
    remote_audit: &RemoteRunnerAudit,
) -> Result<String, HostError> {
    use sha2::Digest;

    let mut entries = Vec::new();
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
            remote_audit,
            &mut entries,
        )
        .await?;
    }
    let bytes = serde_json::to_vec(&entries).map_err(|_| HostError::Identity)?;
    let mut digest = String::with_capacity(64);
    for byte in sha2::Sha256::digest(bytes) {
        use std::fmt::Write as _;
        write!(digest, "{byte:02x}").map_err(|_| HostError::Identity)?;
    }
    Ok(digest)
}

async fn audit_connection(
    docker: &Docker,
    journal: &Journal,
    binding: &RepositoryConnection,
    selected: &SelectedEngineContext,
    poll: &crate::controller_ipc::SessionPollReceipt,
    remote_audit: &RemoteRunnerAudit,
    entries: &mut Vec<InventoryEntry>,
) -> Result<(), HostError> {
    let scope = journal.existing_connection_scope(binding).await?;
    validate_session_scope(&scope, binding, poll).await?;
    for row in scope.physical_runner_reservations().await? {
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
        match row.jit_state {
            PhysicalRunnerJitState::Absent => {
                // Bootstrap records can exist before the start transition.
                // Thus, missing JIT fields do not prove that Docker has no containers.
                return Err(HostError::Ownership);
            }
            PhysicalRunnerJitState::Ready | PhysicalRunnerJitState::Started => {
                if row.reservation.state != ReservationState::Reserved
                    || row.intent_state != Some(IntentState::Pending)
                    || row.completion_cleanup_pending
                    || row.worker_cleanup_proven
                    || row.remote_cleanup_proven
                    || row.local_cleanup_proven
                {
                    return Err(HostError::Ownership);
                }
                let identity = scope
                    .storage_cleanup_identity(row.reservation_id)
                    .await?
                    .ok_or(HostError::Ownership)?;
                audit_started_pair(docker, binding, selected, &row, &identity).await?;
                remote_audit(binding, row.runner.as_ref(), poll.scale_set_id())?;
                entries.push(InventoryEntry::started(
                    binding.connection_id.as_str(),
                    row.reservation_id,
                    assignment,
                    &row,
                    identity.runner_container_id(),
                    identity.dind_container_id(),
                ));
            }
            PhysicalRunnerJitState::Claimed
            | PhysicalRunnerJitState::Uncertain
            | PhysicalRunnerJitState::Bound
            | PhysicalRunnerJitState::StartRequested
            | PhysicalRunnerJitState::StartUncertain
            | PhysicalRunnerJitState::Completed
            | PhysicalRunnerJitState::Cleaned
            | PhysicalRunnerJitState::Unknown => return Err(HostError::Ownership),
        }
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
    inspect_owned_container(
        docker,
        identity.runner_container_id(),
        &names.container_name(WorkerContainerRole::Runner),
        "runner",
        identity,
        binding.connection_id.as_str(),
    )
    .await?;
    inspect_owned_container(
        docker,
        identity.dind_container_id(),
        &names.container_name(WorkerContainerRole::Dind),
        "dind",
        identity,
        binding.connection_id.as_str(),
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
