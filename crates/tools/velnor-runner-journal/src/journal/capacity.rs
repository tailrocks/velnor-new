//! Global launch-capacity reservation with an exact scoped replay identity.

use std::fmt::{self, Formatter};
use std::num::NonZeroU32;

mod assigned;
mod batch;
pub use assigned::{AssignedPopulationObservation, ScopedAssignedLaunchIdentity};
pub use batch::{BatchCapacityClaim, BatchOfferClaim, BatchOfferState};

use crate::error::HostError;

use super::{Journal, one_row};

const SUBJECT_PREFIX: &str = "scope-v1:";

/// GitHub destination and configured Scale Set route used in replay identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayRoute<'a> {
    /// Exact HTTPS API origin, including any configured enterprise prefix.
    pub destination: &'a str,
    /// `repository` or `organization` registration scope.
    pub registration_scope: &'a str,
    /// Exact owner or organization name.
    pub owner: &'a str,
    /// Exact repository name; empty only for organization scope.
    pub repository: &'a str,
    /// Configured runner-group ID.
    pub runner_group_id: i64,
    /// Configured runner-group name.
    pub runner_group_name: &'a str,
    /// Configured Scale Set ID.
    pub scale_set_id: i64,
    /// Configured Scale Set name.
    pub scale_set_name: &'a str,
}

/// Immutable identity for one offer from one exact Scale Set session.
///
/// This key is only for replay isolation. It does not bind a request to a
/// runner or prove that `AcquireJobs` or JIT succeeded.
#[derive(Clone, PartialEq, Eq)]
pub struct ScopedLaunchIdentity {
    subject: String,
}

impl fmt::Debug for ScopedLaunchIdentity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScopedLaunchIdentity")
            .field("subject", &"[redacted]")
            .finish()
    }
}

impl ScopedLaunchIdentity {
    /// Validate and encode the full route, session, message, and request key.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for malformed or oversized identity data.
    pub fn new(
        route: ReplayRoute<'_>,
        session_id: &str,
        message_id: i64,
        request_id: i64,
    ) -> Result<Self, HostError> {
        validate_text(route.destination, 512)?;
        validate_text(route.registration_scope, 16)?;
        validate_text(route.owner, 100)?;
        validate_text(route.runner_group_name, 128)?;
        validate_text(route.scale_set_name, 128)?;
        validate_text(session_id, 256)?;
        if !route.destination.starts_with("https://")
            || !matches!(route.registration_scope, "repository" | "organization")
            || (route.registration_scope == "repository"
                && (route.repository.is_empty() || validate_text(route.repository, 100).is_err()))
            || (route.registration_scope == "organization" && !route.repository.is_empty())
            || route.runner_group_id <= 0
            || route.scale_set_id <= 0
            || message_id < 0
            || request_id <= 0
        {
            return Err(HostError::Journal);
        }

        let parts = [
            route.destination.to_owned(),
            route.registration_scope.to_owned(),
            route.owner.to_owned(),
            route.repository.to_owned(),
            route.runner_group_id.to_string(),
            route.runner_group_name.to_owned(),
            route.scale_set_id.to_string(),
            route.scale_set_name.to_owned(),
            session_id.to_owned(),
            message_id.to_string(),
            request_id.to_string(),
        ];
        let mut subject = String::from(SUBJECT_PREFIX);
        for part in parts {
            append_component(&mut subject, &part);
        }
        Ok(Self { subject })
    }
}

/// Result of atomically checking the global drain gate and launch capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapacityClaim {
    /// A durable reservation was inserted before any external effect.
    New(i64),
    /// This exact scoped offer already has a row; do not repeat effects.
    Existing(i64),
    /// A drain request was committed before this new offer could reserve.
    Draining,
    /// All global permits are occupied by live or unresolved launch rows.
    CapacityFull {
        /// Number of rows that still occupy capacity.
        occupied: u64,
        /// Configured host-wide maximum.
        maximum: NonZeroU32,
    },
}

/// Persisted evidence that launch side effects may have been dispatched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchEffectState {
    /// No effect intent is known; migrated rows use this state.
    Unknown,
    /// A new scoped reservation has not dispatched an effect.
    NotStarted,
    /// An effect intent was durably committed before dispatch.
    MayHaveEffect,
    /// A typed no-effect transition released a scoped reservation.
    DefiniteNoEffect,
}

impl LaunchEffectState {
    pub(super) fn parse(value: &str) -> Result<Self, HostError> {
        match value {
            "unknown" => Ok(Self::Unknown),
            "not_started" => Ok(Self::NotStarted),
            "may_have_effect" => Ok(Self::MayHaveEffect),
            "definite_no_effect" => Ok(Self::DefiniteNoEffect),
            _ => Err(HostError::Journal),
        }
    }
}

impl Journal {
    /// Reserve one global launch slot under the durable drain fence.
    ///
    /// Replay identity includes the exact destination, configured route, full
    /// session ID, message ID, and request ID. Legacy subjects and rows with
    /// unknown effects always occupy capacity. A generic cleanup flag alone
    /// never releases a permit; only a new scoped row recorded as a definite
    /// no-effect failure with no resource or runner observations is excluded.
    ///
    /// The same `BEGIN IMMEDIATE` transaction checks replay, drain, global
    /// occupancy, and persists a new reservation. Callers may run external
    /// effects only after receiving `New`; `Existing`, `Draining`, `CapacityFull`,
    /// and errors must not be acknowledged as newly admitted work.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] on read-only use, invalid state, or
    /// database failure. A failed commit is ambiguous; callers must treat it
    /// as unresolved and may only retry the exact identity.
    pub async fn reserve_launch_if_accepting(
        &self,
        identity: &ScopedLaunchIdentity,
        maximum: NonZeroU32,
    ) -> Result<CapacityClaim, HostError> {
        if self.read_only {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = reserve_in_transaction(&conn, identity, maximum).await;
        let end = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        end.map_err(|_| HostError::Journal)?;
        result
    }

    /// Persist that a launch may perform an external effect.
    ///
    /// Call this and wait for its commit before dispatching Acquire, registration,
    /// JIT, or Docker work. The marker is monotonic; it cannot be reset by
    /// [`Journal::finish`] and keeps the capacity permit occupied.
    ///
    /// Scoped reservations transition from `not_started`; legacy reservations
    /// may transition from `unknown`. Repeated calls are idempotent. The update
    /// also requires the durable drain gate to remain open. If drain wins the
    /// database serialization race, this fails without authorizing the effect;
    /// if the marker wins, the operation is in flight for drain reconciliation.
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] unless this is a pending launch row with
    /// an unresolved marker that can monotonically advance to `may_have_effect`.
    pub async fn record_launch_effect_intent(&self, id: i64) -> Result<(), HostError> {
        if self.read_only {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE intents SET effect_state = 'may_have_effect' WHERE id = ?1 AND kind = 'launch' AND state = 'pending' AND ((replay_key_version = 1 AND effect_state IN ('not_started', 'may_have_effect')) OR (replay_key_version = 0 AND effect_state IN ('unknown', 'may_have_effect'))) AND EXISTS (SELECT 1 FROM controller_state WHERE id = 1 AND draining = 0) AND NOT EXISTS (SELECT 1 FROM worker_cleanup_steps WHERE launch_id = ?1 AND step_key = 'outer-network-removal')",
                [id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        one_row(changed)
    }

    /// Read the monotonic effect marker for an intent.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] if the row is missing or its marker is invalid.
    pub async fn launch_effect_state(&self, id: i64) -> Result<LaunchEffectState, HostError> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query("SELECT effect_state FROM intents WHERE id = ?1", [id])
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let state: String = row.get(0).map_err(|_| HostError::Journal)?;
        LaunchEffectState::parse(&state)
    }

    /// Release a scoped reservation only when no external effect was dispatched.
    ///
    /// This atomic transition is valid only for a new reservation that remains
    /// pending, whose durable effect marker is `not_started`, and that has no
    /// observed runner or resource identity. Callers must commit this before
    /// beginning any external operation. Generic [`Journal::finish`] never
    /// supplies this evidence.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] if the row is legacy, already advanced,
    /// has an observed effect/resource, or the write fails.
    pub async fn record_launch_no_effect(&self, id: i64) -> Result<(), HostError> {
        if self.read_only {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE intents SET state = 'failed', effect_state = 'definite_no_effect' WHERE id = ?1 AND kind = 'launch' AND replay_key_version = 1 AND state = 'pending' AND effect_state = 'not_started' AND docker_id IS NULL AND github_runner_id IS NULL AND dind_id IS NULL AND worker_volume IS NULL AND observed_job_id IS NULL AND observed_workflow_run_id IS NULL AND remote_terminal = 0",
                [id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        one_row(changed)
    }
}

async fn reserve_in_transaction(
    conn: &turso::Connection,
    identity: &ScopedLaunchIdentity,
    maximum: NonZeroU32,
) -> Result<CapacityClaim, HostError> {
    let mut rows = conn
        .query(
            "SELECT id FROM intents WHERE kind = 'launch' AND subject = ?1 ORDER BY id LIMIT 2",
            [identity.subject.as_str()],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let first = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .map(|row| row.get::<i64>(0).map_err(|_| HostError::Journal))
        .transpose()?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    drop(rows);
    if let Some(id) = first {
        return Ok(CapacityClaim::Existing(id));
    }

    let mut rows = conn
        .query("SELECT draining FROM controller_state WHERE id = 1", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let draining = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    drop(rows);
    match draining {
        1 => return Ok(CapacityClaim::Draining),
        0 => {}
        _ => return Err(HostError::Journal),
    }

    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM intents WHERE kind = 'launch' AND cleanup_proven = 0 AND NOT (replay_key_version = 1 AND state = 'failed' AND effect_state = 'definite_no_effect' AND docker_id IS NULL AND github_runner_id IS NULL AND dind_id IS NULL AND worker_volume IS NULL AND observed_job_id IS NULL AND observed_workflow_run_id IS NULL AND remote_terminal = 0)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let occupied = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    drop(rows);
    let occupied = u64::try_from(occupied).map_err(|_| HostError::Journal)?;
    if occupied >= u64::from(maximum.get()) {
        return Ok(CapacityClaim::CapacityFull { occupied, maximum });
    }

    conn.execute(
        "INSERT INTO intents (kind, subject, state, replay_key_version, effect_state, runner_start_state) VALUES ('launch', ?1, 'pending', 1, 'not_started', 'not_requested')",
        [identity.subject.as_str()],
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(CapacityClaim::New(conn.last_insert_rowid()))
}

fn validate_text(value: &str, maximum: usize) -> Result<(), HostError> {
    if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

fn append_component(output: &mut String, value: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    output.push_str(&value.len().to_string());
    output.push('=');
    for byte in value.bytes() {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output.push(';');
}

#[cfg(test)]
mod tests;
