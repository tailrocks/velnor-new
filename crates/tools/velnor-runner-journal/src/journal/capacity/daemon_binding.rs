//! Linux launch admission bound atomically to one trusted logical Docker Engine.

use std::num::NonZeroU32;

use crate::error::HostError;
use crate::journal::daemon_binding::JournalDockerDaemonBinding;
use crate::journal::{Journal, ScopedLaunchIdentity};

use super::assigned::ScopedAssignedLaunchIdentity;

/// Result of reserving capacity together with a Linux Docker Engine binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundCapacityClaim {
    /// A new intent and its binding committed in the same transaction.
    New(i64),
    /// The exact replay row is already bound to this endpoint and Engine ID.
    Existing(i64),
    /// An old row exists but has no durable Engine binding.
    ExistingUnbound(i64),
    /// An old row exists with a different endpoint or Engine ID.
    ExistingBindingChanged(i64),
    /// The durable drain fence prevented a new reservation.
    Draining,
    /// The host-wide capacity is already occupied.
    CapacityFull {
        /// Number of rows that still occupy capacity.
        occupied: u64,
        /// Configured host-wide maximum.
        maximum: NonZeroU32,
    },
}

struct BoundReservation<'a> {
    subject: &'a str,
    replay_key: ReplayKey<'a>,
    message_id: Option<i64>,
}

#[derive(Clone, Copy)]
enum ReplayKey<'a> {
    ExactOffer(&'a str),
    AssignedPrefix(&'a str),
}

impl Journal {
    /// Reserve an available-offer slot and Engine binding atomically.
    ///
    /// An existing row is never rebound. Legacy rows remain occupied and return
    /// `ExistingUnbound`; rows bound to another logical Engine return
    /// `ExistingBindingChanged`. Neither outcome authorizes a Docker request.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] on read-only use or database failure. A
    /// failed commit is ambiguous; retry only this exact identity and binding.
    pub async fn reserve_linux_launch_if_accepting(
        &self,
        identity: &ScopedLaunchIdentity,
        binding: &JournalDockerDaemonBinding,
        maximum: NonZeroU32,
    ) -> Result<BoundCapacityClaim, HostError> {
        reserve_bound(
            self,
            BoundReservation {
                subject: identity.subject.as_str(),
                replay_key: ReplayKey::ExactOffer(identity.subject.as_str()),
                message_id: None,
            },
            binding,
            maximum,
        )
        .await
    }

    /// Reserve an assigned-demand slot and Engine binding atomically.
    ///
    /// An existing row is never rebound. Legacy rows remain occupied and return
    /// `ExistingUnbound`; rows bound to another logical Engine return
    /// `ExistingBindingChanged`. Neither outcome authorizes a Docker request.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] on read-only use or database failure. A
    /// failed commit is ambiguous; retry only this exact identity and binding.
    pub async fn reserve_linux_assigned_launch_if_accepting(
        &self,
        identity: &ScopedAssignedLaunchIdentity,
        binding: &JournalDockerDaemonBinding,
        maximum: NonZeroU32,
    ) -> Result<BoundCapacityClaim, HostError> {
        let (stable_prefix, subject, message_id) = identity.reservation_parts();
        reserve_bound(
            self,
            BoundReservation {
                subject,
                replay_key: ReplayKey::AssignedPrefix(stable_prefix),
                message_id,
            },
            binding,
            maximum,
        )
        .await
    }
}

async fn reserve_bound(
    journal: &Journal,
    reservation: BoundReservation<'_>,
    binding: &JournalDockerDaemonBinding,
    maximum: NonZeroU32,
) -> Result<BoundCapacityClaim, HostError> {
    if journal.read_only {
        return Err(HostError::Journal);
    }
    let conn = journal.connection().await?;
    conn.execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let result = reserve_bound_in_transaction(&conn, reservation, binding, maximum).await;
    let end = if result.is_ok() {
        conn.execute("COMMIT", ()).await
    } else {
        conn.execute("ROLLBACK", ()).await
    };
    end.map_err(|_| HostError::Journal)?;
    result
}

async fn reserve_bound_in_transaction(
    conn: &turso::Connection,
    reservation: BoundReservation<'_>,
    binding: &JournalDockerDaemonBinding,
    maximum: NonZeroU32,
) -> Result<BoundCapacityClaim, HostError> {
    if let Some(launch_id) = existing_launch_id(conn, reservation.replay_key).await? {
        return existing_binding_claim(conn, launch_id, binding).await;
    }
    match super::capacity_admission(conn, maximum).await? {
        super::CapacityAdmission::Draining => return Ok(BoundCapacityClaim::Draining),
        super::CapacityAdmission::Open { occupied, free: 0 } => {
            return Ok(BoundCapacityClaim::CapacityFull { occupied, maximum });
        }
        super::CapacityAdmission::Open { .. } => {}
    }
    let launch_id = insert_bound_intent(conn, reservation, binding).await?;
    Ok(BoundCapacityClaim::New(launch_id))
}

async fn existing_launch_id(
    conn: &turso::Connection,
    replay_key: ReplayKey<'_>,
) -> Result<Option<i64>, HostError> {
    let mut rows = match replay_key {
        ReplayKey::AssignedPrefix(prefix) => {
            conn.query(
                "SELECT id FROM intents WHERE kind = 'launch' AND substr(subject, 1, length(?1)) = ?1 ORDER BY id LIMIT 2",
                [prefix],
            )
            .await
        }
        ReplayKey::ExactOffer(exact) => {
            conn.query(
                "SELECT id FROM intents WHERE kind = 'launch' AND subject = ?1 ORDER BY id LIMIT 2",
                [exact],
            )
            .await
        }
    }
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

async fn insert_bound_intent(
    conn: &turso::Connection,
    reservation: BoundReservation<'_>,
    binding: &JournalDockerDaemonBinding,
) -> Result<i64, HostError> {
    conn.execute(
        "INSERT INTO intents (kind, subject, state, replay_key_version, effect_state, runner_start_state, message_id) VALUES ('launch', ?1, 'pending', 1, 'not_started', 'not_requested', ?2)",
        (reservation.subject, reservation.message_id),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    let launch_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO linux_launch_daemon_bindings (launch_id, endpoint, engine_id) VALUES (?1, ?2, ?3)",
        (launch_id, binding.endpoint(), binding.engine_id()),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(launch_id)
}

async fn existing_binding_claim(
    conn: &turso::Connection,
    launch_id: i64,
    expected: &JournalDockerDaemonBinding,
) -> Result<BoundCapacityClaim, HostError> {
    let mut rows = conn
        .query(
            "SELECT endpoint, engine_id FROM linux_launch_daemon_bindings WHERE launch_id = ?1",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(BoundCapacityClaim::ExistingUnbound(launch_id));
    };
    let endpoint = row.get::<String>(0).map_err(|_| HostError::Journal)?;
    let engine_id = row.get::<String>(1).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    if endpoint == expected.endpoint() && engine_id == expected.engine_id() {
        Ok(BoundCapacityClaim::Existing(launch_id))
    } else {
        Ok(BoundCapacityClaim::ExistingBindingChanged(launch_id))
    }
}
