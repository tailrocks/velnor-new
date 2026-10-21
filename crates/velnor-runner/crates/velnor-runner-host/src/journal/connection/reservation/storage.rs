//! SQL lookups and validation for durable reservations.

use crate::error::HostError;
use crate::journal::sql::token_rejected;
use crate::launch::StartPermit;

use super::super::identity::AssignmentIdentity;
use super::super::storage::scalar_count;
use super::admission::ReservationRequest;

pub(super) fn validate_request(request: ReservationRequest<'_>) -> Result<(), HostError> {
    let assignment_key = request.assignment.storage_key();
    if token_rejected(&assignment_key)
        || assignment_key.len() > 1024
        || token_rejected(request.engine_id)
        || request.engine_id.len() > 128
        || !valid_digest(request.docker_root_digest)
        || request.cpu_millicores == 0
        || request.memory_bytes == 0
        || !valid_generation(request.sample_generation_id)
        || i64::try_from(request.cpu_millicores).is_err()
        || i64::try_from(request.memory_bytes).is_err()
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn valid_generation(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) struct ExistingReservation {
    pub(super) id: i64,
    #[cfg(test)]
    pub(super) intent_id: i64,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) binding_digest: String,
    pub(super) cpu_millicores: i64,
    pub(super) memory_bytes: i64,
    pub(super) released: bool,
    #[cfg(test)]
    pub(super) state: String,
}

pub(super) async fn reservation_for_assignment(
    connection: &turso::Connection,
    connection_id: &str,
    assignment: &AssignmentIdentity,
) -> Result<Option<ExistingReservation>, HostError> {
    let assignment_key = assignment.storage_key();
    let mut rows = connection
        .query(
            "SELECT id, intent_id, engine_id, docker_root_digest, binding_digest, sample_generation_id, cpu_millicores, memory_bytes, state FROM capacity_reservations WHERE connection_id = ?1 AND assignment_key = ?2",
            (connection_id, assignment_key.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    #[cfg(test)]
    let intent_id: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    #[cfg(not(test))]
    let _: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    // Validate the type of the stored sample ID.
    // Production uses the ledger that records sample use.
    let _: Option<String> = row.get(5).map_err(|_| HostError::Journal)?;
    let state: String = row.get(8).map_err(|_| HostError::Journal)?;
    Ok(Some(ExistingReservation {
        id: row.get(0).map_err(|_| HostError::Journal)?,
        #[cfg(test)]
        intent_id,
        engine_id: row.get(2).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(3).map_err(|_| HostError::Journal)?,
        binding_digest: row.get(4).map_err(|_| HostError::Journal)?,
        cpu_millicores: row.get(6).map_err(|_| HostError::Journal)?,
        memory_bytes: row.get(7).map_err(|_| HostError::Journal)?,
        released: state == "released",
        #[cfg(test)]
        state,
    }))
}

pub(super) struct CapacityUsage {
    pub(super) jobs: u32,
    pub(super) cpu_millicores: u64,
    pub(super) memory_bytes: u64,
}

pub(super) struct StoredCapacityLimits {
    pub(super) jobs: u32,
    pub(super) cpu_millicores: u64,
    pub(super) memory_bytes: u64,
    pub(super) operational_cpu_millicores: u64,
    pub(super) operational_memory_bytes: u64,
}

pub(super) async fn capacity_usage(
    connection: &turso::Connection,
) -> Result<CapacityUsage, HostError> {
    capacity_usage_excluding(connection, None).await
}

pub(super) async fn capacity_usage_excluding(
    connection: &turso::Connection,
    excluded_reservation_id: Option<i64>,
) -> Result<CapacityUsage, HostError> {
    let mut rows = connection
        .query(
            "SELECT cpu_millicores, memory_bytes FROM capacity_reservations WHERE state != 'released' AND (?1 IS NULL OR id != ?1)",
            [excluded_reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut usage = CapacityUsage {
        jobs: 0,
        cpu_millicores: 0,
        memory_bytes: 0,
    };
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        usage.jobs = usage.jobs.checked_add(1).ok_or(HostError::Journal)?;
        let cpu = u64::try_from(row.get::<i64>(0).map_err(|_| HostError::Journal)?)
            .map_err(|_| HostError::Journal)?;
        let memory = u64::try_from(row.get::<i64>(1).map_err(|_| HostError::Journal)?)
            .map_err(|_| HostError::Journal)?;
        usage.cpu_millicores = usage
            .cpu_millicores
            .checked_add(cpu)
            .ok_or(HostError::Journal)?;
        usage.memory_bytes = usage
            .memory_bytes
            .checked_add(memory)
            .ok_or(HostError::Journal)?;
    }
    let mut rows = connection
        .query(
            "SELECT COALESCE(SUM(s.retained_memory_bytes), 0) FROM capacity_reservation_storage AS s JOIN capacity_reservations AS r ON r.id = s.reservation_id WHERE r.state != 'released' AND (?1 IS NULL OR r.id != ?1)",
            [excluded_reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let retained_memory = u64::try_from(
        rows.next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<i64>(0)
            .map_err(|_| HostError::Journal)?,
    )
    .map_err(|_| HostError::Journal)?;
    usage.memory_bytes = usage
        .memory_bytes
        .checked_add(retained_memory)
        .ok_or(HostError::Journal)?;
    let mut rows = connection
        .query(
            "SELECT COALESCE(SUM(p.retained_memory_bytes), 0) FROM capacity_reservation_provisioning AS p JOIN capacity_reservations AS r ON r.id = p.reservation_id WHERE r.state != 'released' AND (?1 IS NULL OR r.id != ?1)",
            [excluded_reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let provisioning_memory = u64::try_from(
        rows.next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<i64>(0)
            .map_err(|_| HostError::Journal)?,
    )
    .map_err(|_| HostError::Journal)?;
    usage.memory_bytes = usage
        .memory_bytes
        .checked_add(provisioning_memory)
        .ok_or(HostError::Journal)?;
    Ok(usage)
}

pub(super) async fn capacity_limits(
    connection: &turso::Connection,
) -> Result<StoredCapacityLimits, HostError> {
    let mut rows = connection
        .query(
            "SELECT max_jobs, max_cpu_millicores, max_memory_bytes, operational_cpu_millicores, operational_memory_bytes FROM host_capacity_policy WHERE singleton = 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    Ok(StoredCapacityLimits {
        jobs: u32::try_from(row.get::<i64>(0).map_err(|_| HostError::Journal)?)
            .map_err(|_| HostError::Journal)?,
        cpu_millicores: u64::try_from(row.get::<i64>(1).map_err(|_| HostError::Journal)?)
            .map_err(|_| HostError::Journal)?,
        memory_bytes: u64::try_from(row.get::<i64>(2).map_err(|_| HostError::Journal)?)
            .map_err(|_| HostError::Journal)?,
        operational_cpu_millicores: u64::try_from(
            row.get::<i64>(3).map_err(|_| HostError::Journal)?,
        )
        .map_err(|_| HostError::Journal)?,
        operational_memory_bytes: u64::try_from(row.get::<i64>(4).map_err(|_| HostError::Journal)?)
            .map_err(|_| HostError::Journal)?,
    })
}

pub(super) async fn guest_capacity_fits(
    connection: &turso::Connection,
    permit: &StartPermit,
    request_memory_bytes: u64,
    retained_memory_bytes: u64,
    excluded_reservation_id: Option<i64>,
) -> Result<bool, HostError> {
    let limits = capacity_limits(connection).await?;
    let Some((required_cpu, required_memory)) = StartPermit::checked_guest_capacity(
        limits.cpu_millicores,
        limits.memory_bytes,
        limits.operational_cpu_millicores,
        limits.operational_memory_bytes,
    ) else {
        return Ok(false);
    };
    let usage = capacity_usage_excluding(connection, excluded_reservation_id).await?;
    let Some(available_memory) = crate::launch::StartPermit::required_available_memory(
        usage.memory_bytes,
        request_memory_bytes,
        retained_memory_bytes,
        limits.operational_memory_bytes,
    ) else {
        return Ok(false);
    };
    Ok(permit.has_guest_capacity(required_cpu, required_memory, available_memory))
}

pub(super) async fn has_unreserved_scoped_launch(
    connection: &turso::Connection,
) -> Result<bool, HostError> {
    let count = scalar_count(
        connection,
        "SELECT COUNT(*) FROM connection_intents WHERE kind = 'launch' AND cleanup_proven = 0 AND NOT EXISTS (SELECT 1 FROM capacity_reservations WHERE capacity_reservations.intent_id = connection_intents.id)",
    )
    .await?;
    Ok(count != 0)
}
