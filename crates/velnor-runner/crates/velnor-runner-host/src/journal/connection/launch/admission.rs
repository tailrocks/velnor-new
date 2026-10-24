//! Reservation-backed assignment and external-effect claims.

#[cfg(test)]
use super::super::AssignmentIdentity;
use super::super::ConnectionJournal;
#[cfg(test)]
use super::super::storage::in_transaction;
#[cfg(test)]
use super::helpers::same_runner_identity;
use crate::error::HostError;
#[cfg(test)]
use crate::journal::sql::token_rejected;

impl ConnectionJournal {
    #[cfg(test)]
    pub(crate) async fn begin_launch(
        &self,
        identity: &AssignmentIdentity,
    ) -> Result<(i64, bool), HostError> {
        in_transaction(&self.journal, async |connection| {
            self.validate_assignment(connection, identity).await?;
            let key = identity.storage_key();
            let mut rows = connection
                .query(
                    "SELECT i.id, i.state, i.acquire_attempted, i.jit_requested, i.docker_id, i.dind_id, i.worker_volume, i.launch_phase, r.state FROM connection_intents AS i JOIN capacity_reservations AS r ON r.intent_id = i.id WHERE i.connection_id = ?1 AND i.assignment_key = ?2 AND r.assignment_key = ?2 AND r.state != 'released'",
                    (self.connection_id.as_str(), key.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let row = rows
                .next()
                .await
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?;
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
                return Err(HostError::Journal);
            }
            let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
            let state: String = row.get(1).map_err(|_| HostError::Journal)?;
            let acquire_attempted: bool = row.get(2).map_err(|_| HostError::Journal)?;
            let jit_requested: bool = row.get(3).map_err(|_| HostError::Journal)?;
            let docker_id: Option<String> = row.get(4).map_err(|_| HostError::Journal)?;
            let dind_id: Option<String> = row.get(5).map_err(|_| HostError::Journal)?;
            let volume: Option<String> = row.get(6).map_err(|_| HostError::Journal)?;
            let phase: Option<String> = row.get(7).map_err(|_| HostError::Journal)?;
            let reservation_state: String = row.get(8).map_err(|_| HostError::Journal)?;
            let fresh = state == "pending"
                && reservation_state == "reserved"
                && !acquire_attempted
                && !jit_requested
                && docker_id.is_none()
                && dind_id.is_none()
                && volume.is_none()
                && phase.is_none();
            Ok((id, fresh))
        })
        .await
    }

    /// Return the existing reservation for one acquired request.
    #[cfg(test)]
    pub(crate) async fn begin_assigned_launch(
        &self,
        identity: &AssignmentIdentity,
        runner_name: &str,
    ) -> Result<(i64, bool), HostError> {
        if token_rejected(runner_name) {
            return Err(HostError::Journal);
        }
        let AssignmentIdentity { .. } = identity;
        let Some(request_id) = identity.request_id() else {
            return Err(HostError::Journal);
        };
        if runner_name != format!("v{request_id}") {
            return Err(HostError::Journal);
        }
        let (id, fresh) = self.begin_launch(identity).await?;
        in_transaction(&self.journal, async |connection| {
            self.validate_assignment(connection, identity).await?;
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET runner_request_id = COALESCE(runner_request_id, ?1), runner_name = COALESCE(runner_name, ?2) WHERE id = ?3 AND connection_id = ?4 AND scale_set_id = ?5 AND kind = 'launch' AND cleanup_proven = 0 AND (runner_request_id IS NULL OR runner_request_id = ?1) AND (runner_name IS NULL OR runner_name = ?2)",
                    (request_id, runner_name, id, self.connection_id.as_str(), identity.scale_set_id()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 || same_runner_identity(connection, id, self.connection_id.as_str(), request_id, runner_name).await? {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await?;
        Ok((id, fresh))
    }

    #[cfg(test)]
    pub(in crate::journal::connection) async fn validate_assignment(
        &self,
        connection: &turso::Connection,
        identity: &AssignmentIdentity,
    ) -> Result<(), HostError> {
        self.validate_scope(connection).await?;
        if identity.connection_id() != &self.connection_id
            || self.required_set_id(connection).await? != identity.scale_set_id()
        {
            return Err(HostError::Journal);
        }
        Ok(())
    }

    pub(in crate::journal::connection) async fn required_set_id(
        &self,
        connection: &turso::Connection,
    ) -> Result<i64, HostError> {
        let mut rows = connection
            .query(
                "SELECT scale_set_id FROM connection_runtime_bindings WHERE connection_id = ?1",
                [self.connection_id.as_str()],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        rows.next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<Option<i64>>(0)
            .map_err(|_| HostError::Journal)?
            .filter(|id| *id > 0)
            .ok_or(HostError::Journal)
    }
}
