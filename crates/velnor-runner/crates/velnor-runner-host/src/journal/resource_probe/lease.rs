//! Exclusive host-capacity lease for the resource probe.

use crate::error::HostError;
use crate::journal::resource_probe::ProbeSeed;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeLeaseState {
    Reserved,
    Active,
    Uncertain,
    Recovering,
    Released,
}

impl ProbeLeaseState {
    pub(in crate::journal) fn parse(value: &str) -> Result<Self, HostError> {
        match value {
            "reserved" => Ok(Self::Reserved),
            "active" => Ok(Self::Active),
            "uncertain" => Ok(Self::Uncertain),
            "recovering" => Ok(Self::Recovering),
            "released" => Ok(Self::Released),
            _ => Err(HostError::Journal),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::Active => "active",
            Self::Uncertain => "uncertain",
            Self::Recovering => "recovering",
            Self::Released => "released",
        }
    }
}

pub(in crate::journal) async fn prepare(
    connection: &turso::Connection,
    seed: &ProbeSeed,
) -> Result<(), HostError> {
    prepare_identity(
        connection,
        &seed.operation_id,
        &seed.engine_id,
        &seed.docker_root_digest,
        &seed.operation_name,
    )
    .await
}

pub(in crate::journal) async fn prepare_identity(
    connection: &turso::Connection,
    operation_id: &str,
    engine_id: &str,
    docker_root_digest: &str,
    operation_name: &str,
) -> Result<(), HostError> {
    let charge_cpu =
        i64::try_from(crate::worker::PROBE_CPU_MILLICORES).map_err(|_| HostError::Journal)?;
    let charge_memory =
        i64::try_from(crate::worker::PROBE_MEMORY_LIMIT_BYTES).map_err(|_| HostError::Journal)?;
    let mut policies = connection
        .query(
            "SELECT operational_cpu_millicores, operational_memory_bytes FROM host_capacity_policy WHERE singleton = 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let policy = policies
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let cpu_limit = policy.get::<i64>(0).map_err(|_| HostError::Journal)?;
    let memory_limit = policy.get::<i64>(1).map_err(|_| HostError::Journal)?;
    if cpu_limit < charge_cpu || memory_limit < charge_memory {
        return Err(HostError::Journal);
    }
    let existing = lease_state(connection).await?;
    if existing.is_some_and(|state| state != ProbeLeaseState::Released) {
        return Err(HostError::Journal);
    }
    let prior = connection
        .execute(
            "UPDATE host_operation_leases SET operation_id = ?1, engine_id = ?2, docker_root_digest = ?3, operation_name = ?4, container_id = NULL, cpu_millicores = ?5, memory_bytes = ?6, state = 'reserved' WHERE singleton = 1 AND state = 'released'",
            (
                operation_id,
                engine_id,
                docker_root_digest,
                operation_name,
                charge_cpu,
                charge_memory,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if prior == 0 {
        connection
            .execute(
                "INSERT INTO host_operation_leases (singleton, operation_id, engine_id, docker_root_digest, operation_name, container_id, cpu_millicores, memory_bytes, state) VALUES (1, ?1, ?2, ?3, ?4, NULL, ?5, ?6, 'reserved')",
                (
                    operation_id,
                    engine_id,
                    docker_root_digest,
                    operation_name,
                    charge_cpu,
                    charge_memory,
                ),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(in crate::journal) async fn transition(
    connection: &turso::Connection,
    operation_id: &str,
    next: ProbeLeaseState,
) -> Result<(), HostError> {
    let current = lease_state_for(connection, operation_id)
        .await?
        .ok_or(HostError::Journal)?;
    if current == ProbeLeaseState::Released
        || !matches!(
            next,
            ProbeLeaseState::Active | ProbeLeaseState::Uncertain | ProbeLeaseState::Released
        )
    {
        return Err(HostError::Journal);
    }
    let changed = connection
        .execute(
            "UPDATE host_operation_leases SET state = ?1 WHERE singleton = 1 AND operation_id = ?2 AND state = ?3",
            (next.as_str(), operation_id, current.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed != 1 {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) async fn bind_container(
    connection: &turso::Connection,
    operation_id: &str,
    container_id: &str,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE host_operation_leases SET container_id = ?1, state = 'active' WHERE singleton = 1 AND operation_id = ?2 AND container_id IS NULL AND state IN ('active', 'uncertain', 'recovering')",
            (container_id, operation_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed != 1 {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(in crate::journal) async fn mark_recovering(
    connection: &turso::Connection,
    operation_id: &str,
) -> Result<(), HostError> {
    let current = lease_state_for(connection, operation_id)
        .await?
        .ok_or(HostError::Journal)?;
    if matches!(
        current,
        ProbeLeaseState::Released | ProbeLeaseState::Reserved
    ) {
        return Err(HostError::Journal);
    }
    let changed = connection
        .execute(
            "UPDATE host_operation_leases SET state = 'recovering' WHERE singleton = 1 AND operation_id = ?1 AND state = ?2",
            (operation_id, current.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed != 1 {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) async fn release_prepared(
    connection: &turso::Connection,
    operation_id: &str,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE host_operation_leases SET state = 'released' WHERE singleton = 1 AND operation_id = ?1 AND state = 'reserved' AND container_id IS NULL",
            [operation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) async fn release(
    connection: &turso::Connection,
    operation_id: &str,
    engine_id: &str,
    root_digest: &str,
    operation_name: &str,
    container_id: &str,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE host_operation_leases SET state = 'released' WHERE singleton = 1 AND operation_id = ?1 AND engine_id = ?2 AND docker_root_digest = ?3 AND operation_name = ?4 AND container_id = ?5 AND state IN ('active', 'uncertain', 'recovering')",
            (operation_id, engine_id, root_digest, operation_name, container_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn lease_state(connection: &turso::Connection) -> Result<Option<ProbeLeaseState>, HostError> {
    let mut rows = connection
        .query(
            "SELECT state FROM host_operation_leases WHERE singleton = 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    ProbeLeaseState::parse(&row.get::<String>(0).map_err(|_| HostError::Journal)?).map(Some)
}

async fn lease_state_for(
    connection: &turso::Connection,
    operation_id: &str,
) -> Result<Option<ProbeLeaseState>, HostError> {
    let mut rows = connection
        .query(
            "SELECT state FROM host_operation_leases WHERE singleton = 1 AND operation_id = ?1",
            [operation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    ProbeLeaseState::parse(&row.get::<String>(0).map_err(|_| HostError::Journal)?).map(Some)
}
