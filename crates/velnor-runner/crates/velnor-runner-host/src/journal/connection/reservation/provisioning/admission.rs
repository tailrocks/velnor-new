use std::time::Instant;

use crate::error::HostError;
use crate::journal::connection::AssignmentKey;
use crate::journal::connection::storage::{has_ambiguous_legacy_work, in_transaction};
use crate::worker::{WorkerContainerRole, WorkerIdentityNames, WorkerVolumeRole};

use super::super::admission::{
    ReservationRequest, ReservationResult, ensure_sample_is_new, record_sample_use,
    validate_permit,
};
use super::super::storage::{capacity_limits, capacity_usage, has_unreserved_scoped_launch};
use super::super::storage_budget::{
    StorageInodePolicy, StorageMedium, StorageProfile, StorageProvisioningAdmission,
    StorageProvisioningMemoryObservation,
};
use super::super::storage::ExistingReservation;
use crate::journal::connection::ConnectionJournal;
use crate::launch::StartPermit;

pub(super) async fn reserve(
    journal: &ConnectionJournal,
    request: ReservationRequest<'_>,
    permit: StartPermit,
    admission: StorageProvisioningAdmission,
) -> Result<ReservationResult, HostError> {
    super::super::storage::validate_request(request)?;
    let nonce = reservation_nonce(request.assignment.key())?;
    validate_permit(request, &permit)?;
    let mut permit = Some(permit);
    in_transaction(journal.journal_ref(), async |connection| {
        journal.validate_scope(connection).await?;
        validate_scale_set(connection, journal, request).await?;
        if has_ambiguous_legacy_work(connection).await?
            || has_unreserved_scoped_launch(connection).await?
        {
            return Err(HostError::Journal);
        }
        if let Some(existing) = existing_reservation(connection, journal, request).await? {
            validate_existing(request, journal, &existing)?;
            return Ok(if existing.released {
                ReservationResult::Released {
                    reservation_id: existing.id,
                }
            } else {
                ReservationResult::ReconcileRequired {
                    reservation_id: existing.id,
                }
            });
        }
        if has_legacy_active_reservation(connection).await? {
            return Ok(ReservationResult::AtCapacity);
        }

        let profile = super::super::storage_budget::current_profile(connection)
            .await?
            .ok_or(HostError::Journal)?;
        profile.validate_provisioning_contract()?;
        admission.validate_for(
            &profile,
            request.sample_generation_id,
            request.engine_id,
            request.docker_root_digest,
            request.memory_bytes,
            Instant::now(),
        )?;
        if !disk_pool_fits(connection, &profile, admission.sample()).await?
            || !capacity_fits(
                connection,
                request,
                profile_retained_memory(&profile)?,
            )
            .await?
        {
            return Ok(ReservationResult::AtCapacity);
        }
        ensure_sample_is_new(connection, request.sample_generation_id).await?;
        let permit_value = permit.take().ok_or(HostError::Journal)?;
        if !permit_value.consume_for_admission(request.engine_id, request.docker_root_digest) {
            return Err(HostError::Journal);
        }
        let names = WorkerIdentityNames::from_nonce(nonce)?;
        let runner_name = names.remote_runner_name()?;
        let intent_id = insert_launch_intent(
            connection,
            journal,
            request,
            &runner_name,
        )
        .await?;
        let reservation_id = insert_capacity_reservation(
            connection,
            journal,
            request,
            intent_id,
        )
        .await?;
        record_sample_use(
            connection,
            reservation_id,
            request.sample_generation_id,
            "initial",
        )
        .await?;
        insert_provisioning_rows(
            connection,
            reservation_id,
            intent_id,
            request,
            &profile,
            &admission,
            &names,
        )
        .await?;
        Ok(ReservationResult::Reserved { reservation_id })
    })
    .await
}

async fn has_legacy_active_reservation(
    connection: &turso::Connection,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM capacity_reservations AS r WHERE r.state != 'released' AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning AS p WHERE p.reservation_id = r.id) LIMIT 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}

pub(super) async fn is_v16_reservation(
    connection: &turso::Connection,
    connection_id: &str,
    intent_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM capacity_reservation_provisioning AS p JOIN capacity_reservations AS r ON r.id = p.reservation_id WHERE p.operation_id = ?1 AND r.connection_id = ?2",
            (intent_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}

fn reservation_nonce(key: &AssignmentKey) -> Result<&str, HostError> {
    let AssignmentKey::Remote { kind, subject } = key else {
        return Err(HostError::Identity);
    };
    if kind != "runner"
        || subject.len() != 32
        || !subject
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(HostError::Identity);
    }
    Ok(subject)
}

async fn validate_scale_set(
    connection: &turso::Connection,
    journal: &ConnectionJournal,
    request: ReservationRequest<'_>,
) -> Result<(), HostError> {
    if request.assignment.connection_id() != journal.connection_id() {
        return Err(HostError::Journal);
    }
    let mut rows = connection
        .query(
            "SELECT scale_set_id FROM connection_runtime_bindings WHERE connection_id = ?1",
            [journal.connection_id().as_str()],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let id: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if id == request.assignment.scale_set_id() {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn existing_reservation(
    connection: &turso::Connection,
    journal: &ConnectionJournal,
    request: ReservationRequest<'_>,
) -> Result<Option<ExistingReservation>, HostError> {
    super::super::storage::reservation_for_assignment(
        connection,
        journal.connection_id().as_str(),
        request.assignment,
    )
    .await
}

fn validate_existing(
    request: ReservationRequest<'_>,
    journal: &ConnectionJournal,
    existing: &ExistingReservation,
) -> Result<(), HostError> {
    if existing.engine_id != request.engine_id
        || existing.docker_root_digest != request.docker_root_digest
        || existing.binding_digest != journal.binding_digest()
        || existing.cpu_millicores != i64::try_from(request.cpu_millicores).map_err(|_| HostError::Journal)?
        || existing.memory_bytes != i64::try_from(request.memory_bytes).map_err(|_| HostError::Journal)?
    {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

async fn capacity_fits(
    connection: &turso::Connection,
    request: ReservationRequest<'_>,
    retained_memory: u64,
) -> Result<bool, HostError> {
    let limits = capacity_limits(connection).await?;
    let usage = capacity_usage(connection).await?;
    let requested_memory = request
        .memory_bytes
        .checked_add(retained_memory)
        .ok_or(HostError::Journal)?;
    Ok(usage.jobs < limits.jobs
        && usage
            .cpu_millicores
            .checked_add(request.cpu_millicores)
            .is_some_and(|value| value <= limits.cpu_millicores)
        && usage
            .memory_bytes
            .checked_add(requested_memory)
            .is_some_and(|value| value <= limits.memory_bytes))
}

fn profile_retained_memory(profile: &StorageProfile) -> Result<u64, HostError> {
    profile
        .roles()
        .iter()
        .filter(|role| role.alias_of().is_none())
        .try_fold(0_u64, |sum, role| {
            sum.checked_add(role.retained_memory_bytes())
                .ok_or(HostError::Journal)
        })
}

async fn disk_pool_fits(
    connection: &turso::Connection,
    profile: &StorageProfile,
    sample: &super::super::storage_budget::StorageAdmissionSample,
) -> Result<bool, HostError> {
    let pool = profile
        .pools()
        .iter()
        .find(|pool| pool.medium() == StorageMedium::Disk)
        .ok_or(HostError::Journal)?;
    let observation = sample
        .pools()
        .iter()
        .find(|observation| observation.pool_id() == pool.pool_id())
        .ok_or(HostError::Journal)?;
    if observation.medium() != StorageMedium::Disk
        || observation.total_bytes() < pool.max_bytes()
        || observation.available_bytes() > observation.total_bytes()
        || (pool.inode_policy() == StorageInodePolicy::DynamicBtrfs
            && (observation.filesystem_kind()
                != super::super::storage_budget::StorageFilesystemKind::Btrfs
                || observation.total_inodes() != 0
                || observation.available_inodes() != 0))
    {
        return Ok(false);
    }
    let missing_details = scalar_count(
        connection,
        "SELECT COUNT(*) FROM capacity_reservations AS r WHERE r.state != 'released' AND NOT EXISTS (SELECT 1 FROM capacity_reservation_storage AS s WHERE s.reservation_id = r.id) AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning AS p WHERE p.reservation_id = r.id)",
    )
    .await?;
    if missing_details != 0 {
        return Ok(false);
    }
    let fs_mismatch = scalar_count_with_text(
        connection,
        "SELECT (SELECT COUNT(*) FROM capacity_reservation_storage_roles AS x JOIN capacity_reservations AS r ON r.id = x.reservation_id WHERE r.state != 'released' AND x.pool_id = ?1 AND x.filesystem_instance_digest != ?2) + (SELECT COUNT(*) FROM capacity_reservation_provisioning_pools AS x JOIN capacity_reservations AS r ON r.id = x.reservation_id WHERE r.state != 'released' AND x.pool_id = ?1 AND x.filesystem_instance_digest IS NOT ?2)",
        pool.pool_id(),
        observation.filesystem_instance_digest(),
    )
    .await?;
    if fs_mismatch != 0 {
        return Ok(false);
    }
    let held = sum_held_disk_charge(connection, pool.pool_id()).await?;
    let new_charge = profile
        .roles()
        .iter()
        .filter(|role| role.alias_of().is_none() && role.pool_id() == pool.pool_id())
        .try_fold((0_u64, 0_u64), |sum, role| {
            Ok::<_, HostError>((
                sum.0.checked_add(role.max_bytes()).ok_or(HostError::Journal)?,
                sum.1.checked_add(role.max_inodes()).ok_or(HostError::Journal)?,
            ))
        })?;
    let required_bytes = held
        .0
        .checked_add(new_charge.0)
        .and_then(|sum| sum.checked_add(pool.reserve_bytes()))
        .ok_or(HostError::Journal)?;
    if observation.available_bytes() < required_bytes {
        return Ok(false);
    }
    if pool.inode_policy() == StorageInodePolicy::Finite {
        let required_inodes = held
            .1
            .checked_add(new_charge.1)
            .and_then(|sum| sum.checked_add(pool.reserve_inodes()))
            .ok_or(HostError::Journal)?;
        if observation.total_inodes() < pool.max_inodes()
            || observation.available_inodes() < required_inodes
        {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn sum_held_disk_charge(
    connection: &turso::Connection,
    pool_id: &str,
) -> Result<(u64, u64), HostError> {
    let mut rows = connection
        .query(
            "SELECT COALESCE(SUM(x.charge_bytes), 0), COALESCE(SUM(x.charge_inodes), 0) FROM capacity_reservation_storage_roles AS x JOIN capacity_reservations AS r ON r.id = x.reservation_id WHERE r.state != 'released' AND x.pool_id = ?1",
            [pool_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let mut total = (
        sql_u64(row.get(0).map_err(|_| HostError::Journal)?)?,
        sql_u64(row.get(1).map_err(|_| HostError::Journal)?)?,
    );
    let mut rows = connection
        .query(
            "SELECT COALESCE(SUM(x.charge_bytes), 0), COALESCE(SUM(x.charge_inodes), 0) FROM capacity_reservation_provisioning_roles AS x JOIN capacity_reservations AS r ON r.id = x.reservation_id WHERE r.state != 'released' AND x.pool_id = ?1",
            [pool_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    total.0 = total
        .0
        .checked_add(sql_u64(row.get(0).map_err(|_| HostError::Journal)?)?)
        .ok_or(HostError::Journal)?;
    total.1 = total
        .1
        .checked_add(sql_u64(row.get(1).map_err(|_| HostError::Journal)?)?)
        .ok_or(HostError::Journal)?;
    Ok(total)
}

async fn insert_launch_intent(
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

async fn insert_capacity_reservation(
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

async fn insert_provisioning_rows(
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
    insert_pool_rows(connection, reservation_id, profile, sample).await?;
    insert_memory_row(connection, reservation_id, admission.memory()).await?;
    insert_role_rows(connection, reservation_id, profile, sample).await?;
    insert_object_rows(connection, reservation_id, names).await?;
    insert_container_rows(connection, reservation_id, names).await?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_provisioning_state (reservation_id) VALUES (?1)",
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
    sample: &super::super::storage_budget::StorageAdmissionSample,
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
    memory: &super::super::storage_budget::StorageProvisioningMemoryObservation,
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
    sample: &super::super::storage_budget::StorageAdmissionSample,
) -> Result<(), HostError> {
    for role in super::super::storage_budget::StorageRole::PHYSICAL {
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
                (observed.filesystem_kind(), Some(observed.filesystem_instance_digest()))
            }
            StorageMedium::Tmpfs => (
                super::super::storage_budget::StorageFilesystemKind::Tmpfs,
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
    names: &WorkerIdentityNames,
) -> Result<(), HostError> {
    for worker_role in WorkerVolumeRole::ALL {
        let role = match worker_role {
            WorkerVolumeRole::Socket => super::super::storage_budget::StorageRole::SharedRun,
            WorkerVolumeRole::Home => super::super::storage_budget::StorageRole::RunnerHome,
            WorkerVolumeRole::Work => super::super::storage_budget::StorageRole::RunnerWork,
            WorkerVolumeRole::DindData => super::super::storage_budget::StorageRole::DindData,
        };
        let name = names.volume_name(worker_role);
        connection
            .execute(
                "INSERT INTO capacity_reservation_provisioning_objects (reservation_id, role, docker_name, create_state) VALUES (?1, ?2, ?3, 'unclaimed')",
                (reservation_id, role.as_str(), name),
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

pub(super) const fn container_role(
    role: crate::launch::bootstrap::BootstrapContainerRole,
) -> &'static str {
    match role {
        crate::launch::bootstrap::BootstrapContainerRole::Runner => "runner",
        crate::launch::bootstrap::BootstrapContainerRole::Dind => "dind",
    }
}

async fn scalar_count(connection: &turso::Connection, sql: &str) -> Result<u64, HostError> {
    let mut rows = connection
        .query(sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    let value = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    u64::try_from(value).map_err(|_| HostError::Journal)
}

async fn scalar_count_with_text(
    connection: &turso::Connection,
    sql: &str,
    first: &str,
    second: &str,
) -> Result<u64, HostError> {
    let mut rows = connection
        .query(sql, (first, second))
        .await
        .map_err(|_| HostError::Journal)?;
    let value = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    u64::try_from(value).map_err(|_| HostError::Journal)
}

pub(super) fn sql_i64(value: u64) -> Result<i64, HostError> {
    i64::try_from(value).map_err(|_| HostError::Journal)
}

pub(super) fn sql_u64(value: i64) -> Result<u64, HostError> {
    u64::try_from(value).map_err(|_| HostError::Journal)
}
