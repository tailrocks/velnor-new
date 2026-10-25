//! Provisioning reservation record writes.

use super::{
    ConnectionJournal, HostError, ReservationRequest, StorageMedium, StorageProfile,
    StorageProvisioningAdmission, WorkerContainerRole, WorkerIdentityNames, WorkerVolumeRole,
    profile_retained_memory, sql_i64,
};

pub(super) async fn insert_launch_intent(
    connection: &turso::Connection,
    journal: &ConnectionJournal,
    request: ReservationRequest<'_>,
    runner_name: &str,
) -> Result<i64, HostError> {
    let assignment_key = request.assignment.storage_key();
    connection
        .execute(
            "INSERT INTO connection_intents (connection_id, kind, subject, state, scale_set_id, runner_request_id, runner_name, assignment_key, launch_phase) VALUES (?1, 'launch', ?2, 'pending', ?3, NULL, ?4, ?2, 'provisioning_held')",
            (journal.connection_id().as_str(), assignment_key.as_str(), request.assignment.scale_set_id(), runner_name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(connection.last_insert_rowid())
}

pub(super) async fn insert_capacity_reservation(
    connection: &turso::Connection,
    journal: &ConnectionJournal,
    request: ReservationRequest<'_>,
    intent_id: i64,
) -> Result<i64, HostError> {
    connection
        .execute(
            "INSERT INTO capacity_reservations (intent_id, connection_id, assignment_key, engine_id, docker_root_digest, binding_digest, sample_generation_id, cpu_millicores, memory_bytes, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'reserved')",
            (intent_id, journal.connection_id().as_str(), request.assignment.storage_key(), request.engine_id, request.docker_root_digest, journal.binding_digest(), request.sample_generation_id, sql_i64(request.cpu_millicores)?, sql_i64(request.memory_bytes)?),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(connection.last_insert_rowid())
}

pub(super) async fn insert_provisioning_rows(
    connection: &turso::Connection,
    reservation_id: i64,
    operation_id: i64,
    request: ReservationRequest<'_>,
    profile: &StorageProfile,
    admission: &StorageProvisioningAdmission,
    names: &WorkerIdentityNames,
) -> Result<(), HostError> {
    let sample = admission.sample();
    let retained_memory = profile_retained_memory(profile)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_provisioning (reservation_id, operation_id, profile_generation_id, profile_digest, sample_generation_id, observed_at_unix_ms, engine_id, docker_root_digest, retained_memory_bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            (reservation_id, operation_id, profile.generation_id(), profile.profile_digest(), request.sample_generation_id, sql_i64(sample.observed_at_unix_ms())?, request.engine_id, request.docker_root_digest, sql_i64(retained_memory)?),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_provisioning_topology (reservation_id, topology_version) VALUES (?1, 2)",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    insert_pool_rows(connection, reservation_id, profile, sample).await?;
    insert_memory_row(connection, reservation_id, admission.memory()).await?;
    insert_role_rows(connection, reservation_id, profile, sample).await?;
    insert_object_rows(connection, reservation_id, profile, names).await?;
    insert_container_rows(connection, reservation_id, names).await?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_provisioning_state (reservation_id, bootstrap_start_state) VALUES (?1, 'not_requested')",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn insert_pool_rows(
    connection: &turso::Connection,
    reservation_id: i64,
    profile: &StorageProfile,
    sample: &super::super::super::storage_budget::StorageAdmissionSample,
) -> Result<(), HostError> {
    for pool in profile.pools() {
        if pool.medium() == StorageMedium::Disk {
            let observed = sample
                .pools()
                .iter()
                .find(|row| row.pool_id() == pool.pool_id())
                .ok_or(HostError::Journal)?;
            connection.execute(
                "INSERT INTO capacity_reservation_provisioning_pools (reservation_id, pool_id, scope, medium, filesystem_kind, inode_policy, max_bytes, max_inodes, reserve_bytes, reserve_inodes, filesystem_instance_digest, total_bytes, available_bytes, total_inodes, available_inodes) VALUES (?1, ?2, 'shared-disk', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                (reservation_id, pool.pool_id(), pool.medium().as_str(), observed.filesystem_kind().as_str(), pool.inode_policy().as_str(), sql_i64(pool.max_bytes())?, sql_i64(pool.max_inodes())?, sql_i64(pool.reserve_bytes())?, sql_i64(pool.reserve_inodes())?, observed.filesystem_instance_digest(), sql_i64(observed.total_bytes())?, sql_i64(observed.available_bytes())?, sql_i64(observed.total_inodes())?, sql_i64(observed.available_inodes())?),
            ).await.map_err(|_| HostError::Journal)?;
        } else {
            connection.execute(
                "INSERT INTO capacity_reservation_provisioning_pools (reservation_id, pool_id, scope, medium, filesystem_kind, inode_policy, max_bytes, max_inodes, reserve_bytes, reserve_inodes, filesystem_instance_digest, total_bytes, available_bytes, total_inodes, available_inodes) VALUES (?1, ?2, 'exclusive-tmpfs', ?3, 'tmpfs', ?4, ?5, ?6, ?7, ?8, NULL, NULL, NULL, NULL, NULL)",
                (reservation_id, pool.pool_id(), pool.medium().as_str(), pool.inode_policy().as_str(), sql_i64(pool.max_bytes())?, sql_i64(pool.max_inodes())?, sql_i64(pool.reserve_bytes())?, sql_i64(pool.reserve_inodes())?),
            ).await.map_err(|_| HostError::Journal)?;
        }
    }
    Ok(())
}

async fn insert_memory_row(
    connection: &turso::Connection,
    reservation_id: i64,
    memory: &super::super::super::storage_budget::StorageProvisioningMemoryObservation,
) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO capacity_reservation_provisioning_memory (reservation_id, guest_total_bytes, guest_available_bytes) VALUES (?1, ?2, ?3)",
            (reservation_id, sql_i64(memory.guest_total_bytes())?, sql_i64(memory.guest_available_bytes())?),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn insert_role_rows(
    connection: &turso::Connection,
    reservation_id: i64,
    profile: &StorageProfile,
    sample: &super::super::super::storage_budget::StorageAdmissionSample,
) -> Result<(), HostError> {
    for role in super::super::super::storage_budget::StorageRole::PHYSICAL {
        let limit = profile.role_limit(role)?;
        let pool = profile.pool_limit(limit.pool_id())?;
        let contract = role.contract().ok_or(HostError::Journal)?;
        if contract.medium != limit.medium() {
            return Err(HostError::Journal);
        }
        let (filesystem_kind, filesystem_digest) = match limit.medium() {
            StorageMedium::Disk => {
                let observed = sample
                    .pools()
                    .iter()
                    .find(|row| row.pool_id() == limit.pool_id())
                    .ok_or(HostError::Journal)?;
                (
                    observed.filesystem_kind(),
                    Some(observed.filesystem_instance_digest()),
                )
            }
            StorageMedium::Tmpfs => (
                super::super::super::storage_budget::StorageFilesystemKind::Tmpfs,
                None,
            ),
        };
        connection
            .execute(
                "INSERT INTO capacity_reservation_provisioning_roles (reservation_id, role, pool_id, medium, filesystem_kind, inode_policy, planned_filesystem_instance_digest, charge_bytes, charge_inodes, retained_memory_bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                (reservation_id, role.as_str(), limit.pool_id(), limit.medium().as_str(), filesystem_kind.as_str(), pool.inode_policy().as_str(), filesystem_digest, sql_i64(limit.max_bytes())?, sql_i64(limit.max_inodes())?, sql_i64(limit.retained_memory_bytes())?),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn insert_object_rows(
    connection: &turso::Connection,
    reservation_id: i64,
    profile: &StorageProfile,
    names: &WorkerIdentityNames,
) -> Result<(), HostError> {
    for worker_role in WorkerVolumeRole::ALL {
        let role = match worker_role {
            WorkerVolumeRole::Socket => super::super::super::storage_budget::StorageRole::SharedRun,
            WorkerVolumeRole::Home => super::super::super::storage_budget::StorageRole::RunnerHome,
            WorkerVolumeRole::Work => super::super::super::storage_budget::StorageRole::RunnerWork,
            WorkerVolumeRole::DindData => {
                super::super::super::storage_budget::StorageRole::DindData
            }
            WorkerVolumeRole::RunnerMetadata => {
                super::super::super::storage_budget::StorageRole::EngineMetadata
            }
        };
        let name = names.volume_name(worker_role);
        let policy_digest =
            if role == super::super::super::storage_budget::StorageRole::EngineMetadata {
                Some(
                    profile
                        .controller_publication()
                        .ok_or(HostError::Journal)?
                        .digest(),
                )
            } else {
                None
            };
        connection
            .execute(
                "INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, publication_policy_digest, create_state) VALUES (?1, ?2, ?3, ?4, 'unclaimed')",
                (reservation_id, role.as_str(), name, policy_digest),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn insert_container_rows(
    connection: &turso::Connection,
    reservation_id: i64,
    names: &WorkerIdentityNames,
) -> Result<(), HostError> {
    for (role, worker_role) in [
        (
            crate::launch::bootstrap::BootstrapContainerRole::Runner,
            WorkerContainerRole::Runner,
        ),
        (
            crate::launch::bootstrap::BootstrapContainerRole::Dind,
            WorkerContainerRole::Dind,
        ),
    ] {
        connection
            .execute(
                "INSERT INTO capacity_reservation_bootstrap_containers (reservation_id, role, container_name, create_state, start_state) VALUES (?1, ?2, ?3, 'unclaimed', 'not_requested')",
                (reservation_id, container_role(role), names.container_name(worker_role)),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(in crate::journal::connection::reservation::provisioning) fn container_role(
    role: crate::launch::bootstrap::BootstrapContainerRole,
) -> &'static str {
    match role {
        crate::launch::bootstrap::BootstrapContainerRole::Runner => "runner",
        crate::launch::bootstrap::BootstrapContainerRole::Dind => "dind",
    }
}
