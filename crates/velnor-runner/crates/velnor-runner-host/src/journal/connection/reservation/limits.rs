//! Host-global capacity policy and occupancy queries.

use crate::error::HostError;
use crate::journal::Journal;

use super::super::storage::{has_unresolved_work, in_transaction, read_transaction, scalar_count};

/// Shared host capacity limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CapacityLimits {
    /// Maximum number of active reservations.
    pub(crate) jobs: u32,
    /// Maximum reserved CPU in millicores.
    pub(crate) cpu_millicores: u64,
    /// Maximum reserved memory in bytes.
    pub(crate) memory_bytes: u64,
}

impl Journal {
    /// Set the shared host capacity policy.
    ///
    /// Existing reservations remain charged after a limit decrease.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for invalid limits or a database error.
    pub(crate) async fn configure_capacity(&self, limits: CapacityLimits) -> Result<(), HostError> {
        let jobs = i64::from(limits.jobs);
        let cpu = i64::try_from(limits.cpu_millicores).map_err(|_| HostError::Journal)?;
        let memory = i64::try_from(limits.memory_bytes).map_err(|_| HostError::Journal)?;
        if jobs <= 0 || cpu <= 0 || memory <= 0 {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            connection
                .execute("DELETE FROM host_capacity_policy WHERE singleton = 1", ())
                .await
                .map_err(|_| HostError::Journal)?;
            connection
                .execute(
                    "INSERT INTO host_capacity_policy (singleton, max_jobs, max_cpu_millicores, max_memory_bytes) VALUES (1, ?1, ?2, ?3)",
                    (jobs, cpu, memory),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        })
        .await
    }

    /// Count all work that still occupies or may occupy host capacity.
    ///
    /// Unbound legacy launch rows remain charged.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the journal cannot be read.
    pub(crate) async fn occupied_reservation_count(&self) -> Result<u64, HostError> {
        read_transaction(self, async |connection| {
            Self::occupied_count(connection).await
        })
        .await
    }

    /// Check that no reservation or unresolved journal work remains.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the journal cannot be read.
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
