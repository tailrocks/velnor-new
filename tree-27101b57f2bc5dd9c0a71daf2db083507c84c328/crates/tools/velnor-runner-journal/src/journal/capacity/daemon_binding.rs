//! Linux launch admission bound atomically to one trusted logical Docker Engine.

use std::collections::BTreeSet;
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

/// Atomic result for one batch of scoped, daemon-bound Available identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundBatchCapacityClaim {
    /// Row-level claims in the same order as the input identities.
    ///
    /// This vector contains only `New`, `Existing`, `ExistingUnbound`, or
    /// `ExistingBindingChanged` claims; it never authorizes replayed effects.
    Offers(Vec<BoundCapacityClaim>),
    /// The durable drain fence prevented any new row in this batch.
    Draining,
    /// The remaining global capacity could not fit every new identity.
    CapacityFull {
        /// Number of rows that occupied capacity before this batch.
        occupied: u64,
        /// Fresh identities required by this batch.
        required: u64,
        /// Permits available before this batch.
        available: u64,
        /// Configured host-wide maximum.
        maximum: NonZeroU32,
    },
}

#[derive(Clone, Copy)]
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
        reserve_bound(self, offer_reservation(identity), binding, maximum).await
    }

    /// Atomically reserve every fresh scoped Available identity and bind each
    /// newly inserted row to the supplied trusted logical Docker Engine.
    ///
    /// Replayed rows are returned in input order. Claims describe each row
    /// independently: an `Offers` result may contain `ExistingUnbound` or
    /// `ExistingBindingChanged` for old rows alongside newly bound rows. Such a
    /// result does not mean every offer is bound or launchable, and these claims
    /// do not authorize replayed effects. If the global capacity gate cannot
    /// fit all fresh identities, this method inserts none of them.
    ///
    /// The all-or-none guarantee applies to fresh reservation rows only. It does
    /// not establish a durable multi-offer protocol disposition or authorize a
    /// caller to launch without inspecting every row-level claim.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for an empty, oversized, or duplicate
    /// identity batch, read-only use, or database failure. An ambiguous commit
    /// must be retried only with this exact identity batch and binding.
    pub async fn reserve_linux_launch_batch_all_or_none_if_accepting(
        &self,
        identities: &[ScopedLaunchIdentity],
        binding: &JournalDockerDaemonBinding,
        maximum: NonZeroU32,
    ) -> Result<BoundBatchCapacityClaim, HostError> {
        if self.read_only || identities.is_empty() || identities.len() > 50 {
            return Err(HostError::Journal);
        }
        let subjects: BTreeSet<&str> = identities
            .iter()
            .map(|identity| identity.subject.as_str())
            .collect();
        if subjects.len() != identities.len() {
            return Err(HostError::Journal);
        }

        reserve_bound_batch(self, identities, binding, maximum).await
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
    if let Some(claim) = existing_bound_claim(conn, reservation, binding).await? {
        return Ok(claim);
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

async fn reserve_bound_batch(
    journal: &Journal,
    identities: &[ScopedLaunchIdentity],
    binding: &JournalDockerDaemonBinding,
    maximum: NonZeroU32,
) -> Result<BoundBatchCapacityClaim, HostError> {
    let conn = journal.connection().await?;
    conn.execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let result = reserve_bound_batch_in_transaction(&conn, identities, binding, maximum).await;
    let end = if result.is_ok() {
        conn.execute("COMMIT", ()).await
    } else {
        conn.execute("ROLLBACK", ()).await
    };
    end.map_err(|_| HostError::Journal)?;
    result
}

async fn reserve_bound_batch_in_transaction(
    conn: &turso::Connection,
    identities: &[ScopedLaunchIdentity],
    binding: &JournalDockerDaemonBinding,
    maximum: NonZeroU32,
) -> Result<BoundBatchCapacityClaim, HostError> {
    let mut claims = vec![None; identities.len()];
    let mut fresh = Vec::new();
    for (index, identity) in identities.iter().enumerate() {
        let reservation = offer_reservation(identity);
        match existing_bound_claim(conn, reservation, binding).await? {
            Some(claim) => claims[index] = Some(claim),
            None => fresh.push(index),
        }
    }
    if fresh.is_empty() {
        return collect_batch_claims(claims);
    }

    let (occupied, available) = match super::capacity_admission(conn, maximum).await? {
        super::CapacityAdmission::Draining => return Ok(BoundBatchCapacityClaim::Draining),
        super::CapacityAdmission::Open { occupied, free } => (occupied, free),
    };
    let required = u64::try_from(fresh.len()).map_err(|_| HostError::Journal)?;
    if available < required {
        return Ok(BoundBatchCapacityClaim::CapacityFull {
            occupied,
            required,
            available,
            maximum,
        });
    }

    for index in fresh {
        let launch_id =
            insert_bound_intent(conn, offer_reservation(&identities[index]), binding).await?;
        claims[index] = Some(BoundCapacityClaim::New(launch_id));
    }
    collect_batch_claims(claims)
}

fn collect_batch_claims(
    claims: Vec<Option<BoundCapacityClaim>>,
) -> Result<BoundBatchCapacityClaim, HostError> {
    claims
        .into_iter()
        .map(|claim| claim.ok_or(HostError::Journal))
        .collect::<Result<Vec<_>, _>>()
        .map(BoundBatchCapacityClaim::Offers)
}

fn offer_reservation(identity: &ScopedLaunchIdentity) -> BoundReservation<'_> {
    BoundReservation {
        subject: identity.subject.as_str(),
        replay_key: ReplayKey::ExactOffer(identity.subject.as_str()),
        message_id: None,
    }
}

async fn existing_bound_claim(
    conn: &turso::Connection,
    reservation: BoundReservation<'_>,
    binding: &JournalDockerDaemonBinding,
) -> Result<Option<BoundCapacityClaim>, HostError> {
    let Some(launch_id) = existing_launch_id(conn, reservation.replay_key).await? else {
        return Ok(None);
    };
    existing_binding_claim(conn, launch_id, binding)
        .await
        .map(Some)
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
