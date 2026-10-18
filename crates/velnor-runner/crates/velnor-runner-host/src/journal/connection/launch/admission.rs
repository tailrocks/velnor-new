//! Reservation-backed assignment and external-effect claims.

use super::super::storage::{in_transaction, read_transaction};
use super::super::{AssignmentIdentity, ConnectionJournal};
use super::helpers::same_runner_identity;
use crate::error::HostError;
use crate::journal::source_authorization::{
    AssignedAcquireResult, closed_world_allows_jit, created_set_matches,
    source_authorized_for_intent,
};
use crate::journal::sql::{one_row, token_rejected};

impl ConnectionJournal {
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

    /// Persist a Scale Set and request identity before acquire or JIT.
    pub(crate) async fn bind_launch_identity(
        &self,
        id: i64,
        scale_set_id: i64,
        request_id: Option<i64>,
        runner_name: &str,
    ) -> Result<(), HostError> {
        if id <= 0
            || scale_set_id <= 0
            || request_id.is_some_and(|value| value <= 0)
            || token_rejected(runner_name)
        {
            return Err(HostError::Journal);
        }
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            if self.required_set_id(connection).await? != scale_set_id {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET scale_set_id = COALESCE(scale_set_id, ?1), runner_request_id = COALESCE(runner_request_id, ?2), runner_name = COALESCE(runner_name, ?3) WHERE id = ?4 AND connection_id = ?5 AND kind = 'launch' AND cleanup_proven = 0 AND (scale_set_id IS NULL OR scale_set_id = ?1) AND (?2 IS NULL OR runner_request_id IS NULL OR runner_request_id = ?2) AND (runner_name IS NULL OR runner_name = ?3) AND EXISTS (SELECT 1 FROM capacity_reservations WHERE intent_id = ?4 AND connection_id = ?5 AND state != 'released')",
                    (scale_set_id, request_id, runner_name, id, self.connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            one_row(changed)
        })
        .await
    }

    /// Check whether another live launch owns this runner name in this set.
    pub(crate) async fn launch_identity_taken(
        &self,
        scale_set_id: i64,
        runner_name: &str,
        except_id: i64,
    ) -> Result<bool, HostError> {
        if scale_set_id <= 0 || except_id <= 0 || token_rejected(runner_name) {
            return Err(HostError::Journal);
        }
        read_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            if self.required_set_id(connection).await? != scale_set_id {
                return Err(HostError::Journal);
            }
            let mut rows = connection
                .query(
                    "SELECT id FROM connection_intents WHERE connection_id = ?1 AND kind = 'launch' AND scale_set_id = ?2 AND runner_name = ?3 AND cleanup_proven = 0 AND id != ?4 LIMIT 1",
                    (self.connection_id.as_str(), scale_set_id, runner_name, except_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
        })
        .await
    }

    /// Claim one assigned acquire attempt before its HTTP request.
    pub(crate) async fn claim_assigned_acquire(&self, id: i64) -> Result<bool, HostError> {
        self.claim_launch_flag(id, "acquire").await
    }

    /// Record the definite acquire response.
    pub(crate) async fn record_assigned_acquire(
        &self,
        id: i64,
        result: AssignedAcquireResult,
    ) -> Result<(), HostError> {
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let request_id = assigned_request_id(connection, id, self.connection_id.as_str()).await?;
            let (statement, replay_state) = match &result {
                AssignedAcquireResult::Returned(ids) if ids.as_slice() == [request_id] => (
                    "UPDATE connection_intents SET acquire_resolved = 1, acquired = 1, launch_phase = 'acquired' WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND state = 'pending' AND acquire_attempted = 1 AND acquire_resolved = 0 AND acquired = 0 AND jit_requested = 0 AND cleanup_proven = 0",
                    "acquired",
                ),
                AssignedAcquireResult::Rejected => (
                    "UPDATE connection_intents SET state = 'failed', acquire_resolved = 1, acquired = 0, launch_phase = 'acquire_rejected' WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND state = 'pending' AND acquire_attempted = 1 AND acquire_resolved = 0 AND acquired = 0 AND jit_requested = 0 AND cleanup_proven = 0",
                    "rejected",
                ),
                AssignedAcquireResult::Uncertain | AssignedAcquireResult::Returned(_) => (
                    "UPDATE connection_intents SET state = 'uncertain', launch_phase = 'acquire_uncertain' WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND state = 'pending' AND acquire_attempted = 1 AND acquire_resolved = 0 AND acquired = 0 AND jit_requested = 0 AND cleanup_proven = 0",
                    "uncertain",
                ),
            };
            let changed = connection
                .execute(statement, (id, self.connection_id.as_str()))
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 {
                return Ok(());
            }
            if assigned_acquire_replay_matches(
                connection,
                id,
                self.connection_id.as_str(),
                request_id,
                replay_state,
            )
            .await?
            {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }

    /// Claim one JIT request before its HTTP effect.
    pub(crate) async fn claim_launch_jit(&self, id: i64) -> Result<bool, HostError> {
        self.claim_launch_flag(id, "jit").await
    }

    /// Bind immutable Docker IDs to a scoped launch.
    async fn claim_launch_flag(&self, id: i64, flag: &str) -> Result<bool, HostError> {
        if id <= 0 {
            return Err(HostError::Journal);
        }
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let allowed = match flag {
                "acquire" => {
                    let request_id = assigned_request_id(connection, id, self.connection_id.as_str()).await?;
                    created_set_matches(
                        connection,
                        self.connection_id.as_str(),
                        set_id,
                        self.binding_digest(),
                    )
                    .await?
                        && source_authorized_for_intent(
                            connection,
                            self.connection_id.as_str(),
                            set_id,
                            request_id,
                            id,
                            self.binding_digest(),
                        )
                        .await?
                }
                "jit" => {
                    closed_world_allows_jit(
                        connection,
                        self.connection_id.as_str(),
                        set_id,
                        id,
                        self.binding_digest(),
                    )
                    .await?
                }
                _ => return Err(HostError::Journal),
            };
            if !allowed {
                return Ok(false);
            }
            let statement = match flag {
                "acquire" => "UPDATE connection_intents SET acquire_attempted = 1, launch_phase = 'acquire_requested' WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND scale_set_id IS NOT NULL AND runner_request_id IS NOT NULL AND runner_name IS NOT NULL AND acquire_attempted = 0 AND acquire_resolved = 0 AND acquired = 0 AND jit_requested = 0",
                "jit" => "UPDATE connection_intents SET jit_requested = 1, launch_phase = 'jit_requested' WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND runner_request_id IS NOT NULL AND runner_name IS NOT NULL AND acquire_attempted = 1 AND acquire_resolved = 1 AND acquired = 1 AND jit_requested = 0",
                _ => return Err(HostError::Journal),
            };
            let changed = connection
                .execute(statement, (id, self.connection_id.as_str()))
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(changed == 1)
        })
        .await
    }

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

async fn assigned_request_id(
    connection: &turso::Connection,
    id: i64,
    connection_id: &str,
) -> Result<i64, HostError> {
    let mut rows = connection
        .query(
            "SELECT runner_request_id FROM connection_intents WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND runner_request_id IS NOT NULL AND scale_set_id IS NOT NULL",
            (id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<Option<i64>>(0)
        .map_err(|_| HostError::Journal)?
        .filter(|request_id| *request_id > 0)
        .ok_or(HostError::Journal)
}

async fn assigned_acquire_replay_matches(
    connection: &turso::Connection,
    id: i64,
    connection_id: &str,
    request_id: i64,
    expected: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT state, acquire_attempted, acquire_resolved, acquired, runner_request_id FROM connection_intents WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch'",
            (id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let state: String = row.get(0).map_err(|_| HostError::Journal)?;
    let attempted: bool = row.get(1).map_err(|_| HostError::Journal)?;
    let resolved: bool = row.get(2).map_err(|_| HostError::Journal)?;
    let acquired: bool = row.get(3).map_err(|_| HostError::Journal)?;
    let stored_request_id: Option<i64> = row.get(4).map_err(|_| HostError::Journal)?;
    Ok(attempted
        && stored_request_id == Some(request_id)
        && match expected {
            "acquired" => resolved && acquired && matches!(state.as_str(), "pending" | "done"),
            "rejected" => resolved && !acquired && state == "failed",
            "uncertain" => !resolved && !acquired && state == "uncertain",
            _ => false,
        })
}
