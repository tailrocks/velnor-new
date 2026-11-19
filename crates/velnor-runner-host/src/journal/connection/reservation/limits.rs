// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::header_authority::validate_header_capacity_policy;

use super::super::storage::{has_unresolved_work, in_transaction, read_transaction, scalar_count};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CapacityLimits {
    pub(crate) jobs: u32,

    pub(crate) cpu_millicores: u64,

    pub(crate) memory_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationalCapacityLimits {
    pub(crate) cpu_millicores: u64,

    pub(crate) memory_bytes: u64,
}

impl Journal {
    pub(crate) async fn configure_capacity_with_operational_limits(
        &self,
        limits: CapacityLimits,
        operational: OperationalCapacityLimits,
    ) -> Result<(), HostError> {
        in_transaction(self, async |connection| {
            set_policy(connection, limits, operational).await?;
            validate_header_capacity_policy(connection).await
        })
        .await
    }

    pub(crate) async fn is_drained(&self) -> Result<bool, HostError> {
        read_transaction(self, async |connection| {
            if Self::occupied_count(connection).await? != 0 {
                return Ok(false);
            }
            Ok(!has_unresolved_work(connection).await?)
        })
        .await
    }

    async fn occupied_count(connection: &turso::Connection) -> Result<u64, HostError> {
        let reservations = scalar_count(
            connection,
            "SELECT COUNT(*) FROM capacity_reservations WHERE state != 'released'",
        )
        .await?;
        let legacy = scalar_count(
            connection,
            "SELECT COUNT(*) FROM intents WHERE kind = 'launch' AND cleanup_proven = 0",
        )
        .await?;
        let scoped = scalar_count(
        connection,
        "SELECT COUNT(*) FROM connection_intents WHERE kind = 'launch' AND cleanup_proven = 0 AND NOT EXISTS (SELECT 1 FROM capacity_reservations WHERE capacity_reservations.intent_id = connection_intents.id AND state != 'released')",
    )
    .await?;
        reservations
            .checked_add(legacy)
            .and_then(|value| value.checked_add(scoped))
            .ok_or(HostError::Journal)
    }
}

async fn set_policy(
    connection: &turso::Connection,
    limits: CapacityLimits,
    operational: OperationalCapacityLimits,
) -> Result<(), HostError> {
    let jobs = i64::from(limits.jobs);
    let cpu = i64::try_from(limits.cpu_millicores).map_err(|_| HostError::Journal)?;
    let memory = i64::try_from(limits.memory_bytes).map_err(|_| HostError::Journal)?;
    let operational_cpu =
        i64::try_from(operational.cpu_millicores).map_err(|_| HostError::Journal)?;
    let operational_memory =
        i64::try_from(operational.memory_bytes).map_err(|_| HostError::Journal)?;
    if jobs <= 0 || cpu <= 0 || memory <= 0 || (operational_cpu == 0) != (operational_memory == 0) {
        return Err(HostError::Journal);
    }
    ensure_lease_fits(connection, operational_cpu, operational_memory).await?;
    let exists = capacity_policy_exists(connection).await?;
    if exists {
        connection
            .execute(
                "UPDATE host_capacity_policy SET max_jobs = ?1, max_cpu_millicores = ?2, max_memory_bytes = ?3, operational_cpu_millicores = ?4, operational_memory_bytes = ?5 WHERE singleton = 1",
                (jobs, cpu, memory, operational_cpu, operational_memory),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    } else {
        connection
            .execute(
                "INSERT INTO host_capacity_policy (singleton, max_jobs, max_cpu_millicores, max_memory_bytes, operational_cpu_millicores, operational_memory_bytes) VALUES (1, ?1, ?2, ?3, ?4, ?5)",
                (jobs, cpu, memory, operational_cpu, operational_memory),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn capacity_policy_exists(connection: &turso::Connection) -> Result<bool, HostError> {
    let mut rows = connection
        .query("SELECT 1 FROM host_capacity_policy WHERE singleton = 1", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}

async fn ensure_lease_fits(
    connection: &turso::Connection,
    cpu_limit: i64,
    memory_limit: i64,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT cpu_millicores, memory_bytes FROM host_operation_leases WHERE singleton = 1 AND state != 'released'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(());
    };
    let cpu = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
    let memory = row.get::<i64>(1).map_err(|_| HostError::Journal)?;
    if cpu > cpu_limit || memory > memory_limit {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}
