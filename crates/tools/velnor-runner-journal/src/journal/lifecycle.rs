//! Durable worker network identity and runner-start intent.

use crate::error::HostError;

use super::{Journal, one_row, token_rejected};

/// Durable state immediately before a runner start call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerStartIntent {
    /// An older row has no trustworthy start history.
    UnknownLegacy,
    /// The current row has not dispatched the runner start call.
    NotRequested,
    /// The runner start call may have reached Docker.
    MayHaveStarted,
}

impl RunnerStartIntent {
    pub(super) fn parse(value: &str) -> Result<Self, HostError> {
        match value {
            "unknown_legacy" => Ok(Self::UnknownLegacy),
            "not_requested" => Ok(Self::NotRequested),
            "may_have_started" => Ok(Self::MayHaveStarted),
            _ => Err(HostError::Journal),
        }
    }
}

impl Journal {
    /// Persist the deterministic private-network name before Docker creates it.
    ///
    /// The name is immutable once recorded. The launch must already have a
    /// durable may-have-effect marker so a crash cannot free its capacity.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the name, row state, or durable
    /// effect marker is invalid, or the database write fails.
    pub async fn record_outer_network_intent(
        &self,
        launch_id: i64,
        network_name: &str,
    ) -> Result<(), HostError> {
        if self.read_only || !valid_network_name(network_name) {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE intents SET outer_network_name = ?1 WHERE id = ?2 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND effect_state = 'may_have_effect' AND (outer_network_name IS NULL OR outer_network_name = ?1)",
                (network_name.to_owned(), launch_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        one_row(changed)
    }

    /// Bind the exact network ID after Docker has returned and verified it.
    ///
    /// This requires a prior durable name intent. An unknown create outcome
    /// leaves only the name, which remains occupied for later exact-name
    /// reconciliation.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when no prior name intent exists, the
    /// ID conflicts with an earlier binding, or the database write fails.
    pub async fn bind_outer_network_id(
        &self,
        launch_id: i64,
        network_id: &str,
    ) -> Result<(), HostError> {
        if self.read_only || !valid_docker_id(network_id) {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE intents SET outer_network_id = COALESCE(outer_network_id, ?1) WHERE id = ?2 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND effect_state = 'may_have_effect' AND outer_network_name IS NOT NULL AND (outer_network_id IS NULL OR outer_network_id = ?1)",
                (network_id.to_owned(), launch_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        one_row(changed)
    }

    /// Persist that runner start may occur immediately before calling Docker.
    ///
    /// The container ID must already be durably bound to this launch. The
    /// marker is monotonic: retries with the same ID are idempotent, but a
    /// different ID or a migrated unknown row is rejected.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the exact runner ID is not bound,
    /// the row is not safely pending, or the database write fails.
    pub async fn record_runner_start_intent(
        &self,
        launch_id: i64,
        runner_container_id: &str,
    ) -> Result<(), HostError> {
        if self.read_only || !valid_docker_id(runner_container_id) {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE intents SET runner_start_state = 'may_have_started' WHERE id = ?1 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND effect_state = 'may_have_effect' AND docker_id = ?2 AND runner_start_state IN ('not_requested', 'may_have_started')",
                (launch_id, runner_container_id.to_owned()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        one_row(changed)
    }

    /// Read the persisted runner-start intent after reopening the journal.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is missing or corrupt.
    pub async fn runner_start_intent(
        &self,
        launch_id: i64,
    ) -> Result<RunnerStartIntent, HostError> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT runner_start_state FROM intents WHERE id = ?1 AND kind = 'launch'",
                [launch_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        RunnerStartIntent::parse(&row.get::<String>(0).map_err(|_| HostError::Journal)?)
    }
}

fn valid_network_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !token_rejected(name)
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_docker_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && !token_rejected(id)
        && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests;
