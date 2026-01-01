//! Durable reservations for generic JIT slots derived from assigned population.

use std::num::NonZeroU32;

use super::{CapacityClaim, ReplayRoute, append_component, validate_text};
use crate::error::HostError;
use crate::journal::Journal;

const SUBJECT_PREFIX: &str = "assigned-slot-v1:";

/// A source-bound assigned/running population sample used for a generic JIT slot.
///
/// This value carries no workflow, request, or runner identity. It is audit
/// context for an opaque assigned-demand permit; the stable replay key is the
/// session, demand identifier, and ordinal, so changed counts cannot authorize
/// a second row for the same slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssignedPopulationObservation {
    observed_at_ms: u64,
    assigned_jobs: u32,
    running_jobs: u32,
}

impl AssignedPopulationObservation {
    /// Validate one positive-demand population observation.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for a zero timestamp or inconsistent
    /// assigned/running counts.
    pub fn new(
        observed_at_ms: u128,
        assigned_jobs: u32,
        running_jobs: u32,
    ) -> Result<Self, HostError> {
        let observed_at_ms = u64::try_from(observed_at_ms).map_err(|_| HostError::Journal)?;
        if observed_at_ms == 0 || assigned_jobs <= running_jobs {
            return Err(HostError::Journal);
        }
        Ok(Self {
            observed_at_ms,
            assigned_jobs,
            running_jobs,
        })
    }

    /// Millisecond timestamp captured when the service statistics were read.
    #[must_use]
    pub const fn observed_at_ms(self) -> u64 {
        self.observed_at_ms
    }

    /// Total assigned jobs from the same service response.
    #[must_use]
    pub const fn assigned_jobs(self) -> u32 {
        self.assigned_jobs
    }

    /// Total running jobs from the same service response.
    #[must_use]
    pub const fn running_jobs(self) -> u32 {
        self.running_jobs
    }
}

/// Stable idempotency identity for one generic assigned-demand JIT ordinal.
///
/// Repository names and population counts are retained only as row audit
/// context. The stable prefix uses the immutable repository ID and exact
/// session/demand/ordinal, so a rename or changed statistics sample cannot
/// replay a slot as new work.
#[derive(Clone, PartialEq, Eq)]
pub struct ScopedAssignedLaunchIdentity {
    stable_prefix: String,
    subject: String,
    message_id: Option<i64>,
}

impl std::fmt::Debug for ScopedAssignedLaunchIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScopedAssignedLaunchIdentity")
            .field("stable_prefix", &"[redacted]")
            .field("subject", &"[redacted]")
            .field("message_id", &self.message_id)
            .finish()
    }
}

impl ScopedAssignedLaunchIdentity {
    /// Validate and bind one exact route/session/demand ordinal.
    ///
    /// `target_repository_id` remains stable across repository renames and is
    /// mandatory even for organization registration scope. `message_id` is
    /// optional because population statistics may arrive with session create.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for malformed route, session, demand,
    /// observation, or ordinal data.
    pub fn new(
        route: ReplayRoute<'_>,
        target_repository_id: i64,
        session_id: &str,
        demand_id: u64,
        message_id: Option<i64>,
        observation: AssignedPopulationObservation,
        ordinal: u64,
    ) -> Result<Self, HostError> {
        validate_route(
            route,
            target_repository_id,
            session_id,
            demand_id,
            message_id,
        )?;

        let stable_parts = [
            route.destination.to_owned(),
            route.registration_scope.to_owned(),
            target_repository_id.to_string(),
            route.runner_group_id.to_string(),
            route.scale_set_id.to_string(),
            session_id.to_owned(),
            demand_id.to_string(),
            ordinal.to_string(),
        ];
        let mut stable_prefix = SUBJECT_PREFIX.to_owned();
        for part in stable_parts {
            append_component(&mut stable_prefix, &part);
        }

        let audit_parts = [
            route.owner.to_owned(),
            route.repository.to_owned(),
            route.runner_group_name.to_owned(),
            route.scale_set_name.to_owned(),
            message_id.map_or_else(|| "none".to_owned(), |value| value.to_string()),
            observation.observed_at_ms.to_string(),
            observation.assigned_jobs.to_string(),
            observation.running_jobs.to_string(),
        ];
        let mut subject = stable_prefix.clone();
        for part in audit_parts {
            append_component(&mut subject, &part);
        }

        Ok(Self {
            stable_prefix,
            subject,
            message_id,
        })
    }

    pub(super) fn reservation_parts(&self) -> (&str, &str, Option<i64>) {
        (
            self.stable_prefix.as_str(),
            self.subject.as_str(),
            self.message_id,
        )
    }
}

impl Journal {
    /// Reserve one generic assigned-demand slot before JIT or Docker effects.
    ///
    /// The transaction checks stable replay identity, the durable drain fence,
    /// and global occupancy together. Existing rows never authorize replay,
    /// even when the caller presents a later statistics sample.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] on read-only use, duplicate stored
    /// identities, or database failure. Ambiguous commits must be held.
    pub async fn reserve_assigned_launch_if_accepting(
        &self,
        identity: &ScopedAssignedLaunchIdentity,
        maximum: NonZeroU32,
    ) -> Result<CapacityClaim, HostError> {
        if self.read_only {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = reserve_assigned_in_transaction(&conn, identity, maximum).await;
        let end = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        end.map_err(|_| HostError::Journal)?;
        result
    }
}

async fn reserve_assigned_in_transaction(
    conn: &turso::Connection,
    identity: &ScopedAssignedLaunchIdentity,
    maximum: NonZeroU32,
) -> Result<CapacityClaim, HostError> {
    if let Some(id) = existing_assigned_slot(conn, identity).await? {
        return Ok(CapacityClaim::Existing(id));
    }
    match super::capacity_admission(conn, maximum).await? {
        super::CapacityAdmission::Draining => return Ok(CapacityClaim::Draining),
        super::CapacityAdmission::Open { occupied, free: 0 } => {
            return Ok(CapacityClaim::CapacityFull { occupied, maximum });
        }
        super::CapacityAdmission::Open { .. } => {}
    }
    conn.execute(
        "INSERT INTO intents (kind, subject, state, replay_key_version, effect_state, runner_start_state, message_id) VALUES ('launch', ?1, 'pending', 1, 'not_started', 'not_requested', ?2)",
        (identity.subject.as_str(), identity.message_id),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(CapacityClaim::New(conn.last_insert_rowid()))
}

async fn existing_assigned_slot(
    conn: &turso::Connection,
    identity: &ScopedAssignedLaunchIdentity,
) -> Result<Option<i64>, HostError> {
    let mut rows = conn
        .query(
            "SELECT id FROM intents WHERE kind = 'launch' AND substr(subject, 1, length(?1)) = ?1 ORDER BY id LIMIT 2",
            [identity.stable_prefix.as_str()],
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
    Ok(first)
}

fn validate_route(
    route: ReplayRoute<'_>,
    target_repository_id: i64,
    session_id: &str,
    demand_id: u64,
    message_id: Option<i64>,
) -> Result<(), HostError> {
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
        || target_repository_id <= 0
        || route.runner_group_id <= 0
        || route.scale_set_id <= 0
        || demand_id == 0
        || message_id.is_some_and(|value| value < 0)
    {
        return Err(HostError::Journal);
    }
    Ok(())
}
