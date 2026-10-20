use crate::error::HostError;
use crate::journal::connection::storage::in_transaction;
use crate::journal::connection::{ConnectionJournal, StorageRole};
use crate::launch::bootstrap::VerifiedStorageCleanup;
use crate::worker::{WorkerIdentityNames, WorkerVolumeRole};
use std::collections::BTreeMap;

use super::{HoldContext, hold_context};
use record_validation::{storage_cleanup_already_proven, validate_existing_cleanup};

#[path = "cleanup/identity.rs"]
mod identity;
#[path = "cleanup/record_validation.rs"]
mod record_validation;
#[path = "cleanup/terminal.rs"]
mod terminal;
pub(in crate::journal) use terminal::validate_retirement_digest;
#[path = "cleanup/witness.rs"]
mod witness;
pub(crate) use identity::StorageCleanupIdentity;
#[cfg(test)]
pub(crate) use identity::{StorageCleanupCgroup, StorageCleanupMount};
pub(crate) use witness::StorageCleanupObserverWitness;

pub(super) async fn post_jit_retirement_claim_exists(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM physical_attempt_post_jit_retirement_claims WHERE reservation_id=?1 LIMIT 1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}

pub(super) async fn record(
    journal: &ConnectionJournal,
    proof: VerifiedStorageCleanup,
) -> Result<(), HostError> {
    if !proof.all_owned_refs_absent() || !valid_roles(proof.role_inventory()) {
        return Err(HostError::Identity);
    }
    let connection_id = journal.connection_id().as_str().to_owned();
    let reservation_id = proof.reservation_id();
    if reservation_id <= 0 {
        return Err(HostError::Identity);
    }
    in_transaction(journal.journal_ref(), async |connection| {
        journal.validate_scope(connection).await?;
        let Some(context) = hold_context(connection, &connection_id, reservation_id).await? else {
            return Err(HostError::Journal);
        };
        if context.operation_id != proof.operation_id()
            || context.engine_id != proof.engine_id()
            || context.docker_root_digest != proof.docker_root_digest()
        {
            return Err(HostError::Identity);
        }
        let binding = bound_identity(connection, reservation_id).await?;
        let inventory = cleanup_inventory(connection, reservation_id).await?;
        if binding.runner_id != proof.runner_container_id()
            || binding.dind_id != proof.dind_container_id()
            || binding.instance_binding_digest != proof.instance_binding_digest()
            || proof.volume_names() != inventory.volume_names
        {
            return Err(HostError::Identity);
        }
        let metadata_volume = inventory.volume_names.get(4).map(String::as_str);
        let changed = connection
            .execute(
                "INSERT INTO capacity_reservation_provisioning_cleanups (reservation_id, runner_container_id, dind_container_id, shared_run_volume, runner_home_volume, runner_work_volume, dind_data_volume, metadata_volume, instance_binding_digest, role_mask, containers_absent, volumes_absent, mounts_detached, cgroups_absent) SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 2047, 1, 1, 1, 1 WHERE NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_cleanups WHERE reservation_id = ?1)",
                (
                    reservation_id,
                    proof.runner_container_id(),
                    proof.dind_container_id(),
                    inventory.volume_names[0].as_str(),
                    inventory.volume_names[1].as_str(),
                    inventory.volume_names[2].as_str(),
                    inventory.volume_names[3].as_str(),
                    metadata_volume,
                    proof.instance_binding_digest(),
                ),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed == 0 {
            validate_existing_cleanup(
                connection,
                reservation_id,
                proof.runner_container_id(),
                proof.dind_container_id(),
                &inventory.volume_names,
                proof.instance_binding_digest(),
            )
            .await?;
        } else if changed != 1 {
            return Err(HostError::Journal);
        }
        let changed = connection
            .execute(
                "UPDATE capacity_reservation_provisioning_state SET storage_cleanup_proven = 1 WHERE reservation_id = ?1 AND resources_bound = 1 AND instance_binding_digest = ?2 AND storage_cleanup_proven = 0",
                (reservation_id, proof.instance_binding_digest()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed == 0 && !storage_cleanup_already_proven(connection, reservation_id).await? {
            return Err(HostError::Journal);
        }
        settle_if_fully_cleaned(connection, &connection_id, reservation_id, &context).await
    })
    .await
}

pub(super) async fn insert_observer_witness(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
    binding: &crate::launch::bootstrap::VerifiedStorageResourceBinding,
) -> Result<(), HostError> {
    witness::insert(connection, connection_id, reservation_id, binding).await
}

pub(super) async fn read_observer_witness(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
) -> Result<Option<StorageCleanupObserverWitness>, HostError> {
    witness::read(connection, connection_id, reservation_id).await
}

async fn bound_identity(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<BoundIdentity, HostError> {
    let mut rows = connection
        .query(
            "SELECT runner_container_id, dind_container_id, instance_binding_digest FROM capacity_reservation_provisioning_state WHERE reservation_id = ?1 AND bootstrap_start_state = 'started' AND resources_bound = 1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    Ok(BoundIdentity {
        runner_id: row.get(0).map_err(|_| HostError::Journal)?,
        dind_id: row.get(1).map_err(|_| HostError::Journal)?,
        instance_binding_digest: row.get(2).map_err(|_| HostError::Journal)?,
    })
}

struct BoundIdentity {
    runner_id: String,
    dind_id: String,
    instance_binding_digest: String,
}

async fn names_for_reservation(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<WorkerIdentityNames, HostError> {
    let mut rows = connection
        .query(
            "SELECT i.runner_name FROM connection_intents AS i JOIN capacity_reservations AS r ON r.intent_id = i.id AND r.connection_id = i.connection_id WHERE r.id = ?1 AND i.kind = 'launch'",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let runner_name: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    let nonce = runner_name.strip_prefix('v').ok_or(HostError::Journal)?;
    WorkerIdentityNames::from_nonce(nonce)
}

struct CleanupInventory {
    volume_names: Vec<String>,
}

async fn cleanup_inventory(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<CleanupInventory, HostError> {
    let mut topology_rows = connection
        .query(
            "SELECT topology_version FROM capacity_reservation_provisioning_topology WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let topology_version: i64 = topology_rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    let expected_roles: &[(StorageRole, WorkerVolumeRole)] = match topology_version {
        1 => &[
            (StorageRole::SharedRun, WorkerVolumeRole::Socket),
            (StorageRole::RunnerHome, WorkerVolumeRole::Home),
            (StorageRole::RunnerWork, WorkerVolumeRole::Work),
            (StorageRole::DindData, WorkerVolumeRole::DindData),
        ],
        2 => &[
            (StorageRole::SharedRun, WorkerVolumeRole::Socket),
            (StorageRole::RunnerHome, WorkerVolumeRole::Home),
            (StorageRole::RunnerWork, WorkerVolumeRole::Work),
            (StorageRole::DindData, WorkerVolumeRole::DindData),
            (
                StorageRole::EngineMetadata,
                WorkerVolumeRole::RunnerMetadata,
            ),
        ],
        _ => return Err(HostError::Journal),
    };
    let expected_names = names_for_reservation(connection, reservation_id).await?;
    let mut rows = connection
        .query(
            "SELECT role, docker_name, docker_id, create_state, publication_policy_digest FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut objects = BTreeMap::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let role: String = row.get(0).map_err(|_| HostError::Journal)?;
        let docker_name: String = row.get(1).map_err(|_| HostError::Journal)?;
        let docker_id: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
        let create_state: String = row.get(3).map_err(|_| HostError::Journal)?;
        let publication_policy: Option<String> = row.get(4).map_err(|_| HostError::Journal)?;
        if objects
            .insert(
                role,
                (docker_name, docker_id, create_state, publication_policy),
            )
            .is_some()
        {
            return Err(HostError::Journal);
        }
    }
    if objects.len() != expected_roles.len() {
        return Err(HostError::Journal);
    }
    let mut volume_names = Vec::with_capacity(expected_roles.len());
    for (role, worker_role) in expected_roles {
        let (docker_name, docker_id, create_state, publication_policy) =
            objects.get(role.as_str()).ok_or(HostError::Journal)?;
        if *docker_name != expected_names.volume_name(*worker_role)
            || docker_id.as_deref() != Some(docker_name.as_str())
            || create_state != "created"
            || (*role == StorageRole::EngineMetadata && publication_policy.is_none())
        {
            return Err(HostError::Journal);
        }
        volume_names.push(docker_name.clone());
    }
    Ok(CleanupInventory { volume_names })
}

async fn settle_if_fully_cleaned(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
    context: &HoldContext,
) -> Result<(), HostError> {
    if post_jit_retirement_claim_exists(connection, reservation_id).await? {
        return Ok(());
    }
    let mut rows = connection
        .query(
            "SELECT remote_cleanup_proven, local_cleanup_proven, state FROM capacity_reservations WHERE id = ?1 AND connection_id = ?2",
            (reservation_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let remote: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let local: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let state: String = row.get(2).map_err(|_| HostError::Journal)?;
    if remote != 1 || local != 1 {
        return Ok(());
    }
    if state == "released" {
        return Ok(());
    }
    crate::journal::request_lifecycle::physical::mark_runner_cleaned_after_cleanup(
        connection,
        connection_id,
        context.operation_id,
    )
    .await?;
    if !super::release_ready(connection, reservation_id).await? {
        return Ok(());
    }
    prove_cleanup_before_release(
        connection,
        connection_id,
        context.operation_id,
        reservation_id,
        false,
    )
    .await
}

pub(super) async fn prove_cleanup_before_release(
    connection: &turso::Connection,
    connection_id: &str,
    operation_id: i64,
    reservation_id: i64,
    allow_existing_proof: bool,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE connection_intents SET cleanup_proven = 1 WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND cleanup_proven = 0",
            (operation_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 0 && allow_existing_proof {
        let already_proven = connection
            .query(
                "SELECT 1 FROM connection_intents WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND cleanup_proven = 1",
                (operation_id, connection_id),
            )
            .await
            .map_err(|_| HostError::Journal)?
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .is_some();
        if !already_proven {
            return Err(HostError::Journal);
        }
    } else if changed != 1 {
        return Err(HostError::Journal);
    }
    terminal::settle_reservation(connection, connection_id, operation_id, reservation_id).await
}

pub(super) fn valid_roles(roles: &[StorageRole]) -> bool {
    roles.len() == StorageRole::PHYSICAL.len()
        && StorageRole::PHYSICAL
            .iter()
            .all(|role| roles.iter().filter(|candidate| *candidate == role).count() == 1)
}
