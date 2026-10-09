//! Atomic bounded queue-offer capacity reservations.

use std::collections::BTreeSet;
use std::num::NonZeroU32;

use super::super::Journal;
use crate::error::HostError;

/// Durable state of one offered ID in an atomically admitted queue batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchOfferState {
    /// A fresh capacity row was committed before any acquire effect.
    Reserved {
        /// Durable launch row.
        launch_id: i64,
    },
    /// A prior reservation is still durably `not_started`; it is safe to
    /// finish identity binding and dispatch the first effect.
    Ready {
        /// Durable launch row.
        launch_id: i64,
    },
    /// A prior row exists. This primitive does not authorize replay based on
    /// its later launch or cleanup state.
    Existing {
        /// Durable launch row.
        launch_id: i64,
    },
    /// Capacity or this call's selection bound prevented a new reservation.
    Deferred,
}

/// One offered request and the journal state that governs its replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchOfferClaim {
    /// Queue `runnerRequestId`; this is an offer identity, not a runner binding.
    pub request_id: i64,
    /// Durable state of this offer's capacity slot.
    pub state: BatchOfferState,
}

/// Atomically classified every bounded offer and reserved as many free slots as requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchCapacityClaim {
    /// Per-offer results in input order. Fresh rows are inserted as one transaction.
    Offers(Vec<BatchOfferClaim>),
    /// Drain was committed before any fresh row could be reserved.
    Draining,
}

impl Journal {
    /// Atomically reserve a bounded subset of fresh offers in one queue batch.
    ///
    /// Rows use the same exact per-offer subject as the legacy single-offer
    /// path (`m{message_id}r{request_id}`), so changing between single and
    /// multi-offer handling cannot replay an already reserved ID. Existing
    /// offer rows are never re-authorized by this primitive, while later,
    /// previously unseen offers in the same message may use free capacity.
    /// `maximum_new` bounds this
    /// turn's selection; the global capacity and drain checks share the same
    /// transaction as every new row.
    ///
    /// The request ID identifies only the offer/capacity slot. It does not
    /// associate a request with a particular JIT or runner.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for malformed/duplicate request IDs, more
    /// than 50 offers, read-only use, or database failure. If the commit outcome
    /// is ambiguous, callers must not dispatch an effect and must treat replay
    /// as existing/unknown.
    pub async fn reserve_launch_batch_if_accepting(
        &self,
        message_id: i64,
        request_ids: &[i64],
        maximum_new: usize,
        maximum: NonZeroU32,
    ) -> Result<BatchCapacityClaim, HostError> {
        if self.read_only
            || message_id < 0
            || request_ids.is_empty()
            || request_ids.len() > 50
            || maximum_new > request_ids.len()
            || request_ids.iter().any(|request_id| *request_id <= 0)
        {
            return Err(HostError::Journal);
        }
        let distinct: BTreeSet<i64> = request_ids.iter().copied().collect();
        if distinct.len() != request_ids.len() {
            return Err(HostError::Journal);
        }

        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result =
            reserve_batch_in_transaction(&conn, message_id, request_ids, maximum_new, maximum)
                .await;
        let end = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        end.map_err(|_| HostError::Journal)?;
        result
    }
}

async fn reserve_batch_in_transaction(
    conn: &turso::Connection,
    message_id: i64,
    request_ids: &[i64],
    maximum_new: usize,
    maximum: NonZeroU32,
) -> Result<BatchCapacityClaim, HostError> {
    let mut free = match super::capacity_admission(conn, maximum).await? {
        super::CapacityAdmission::Draining => return Ok(BatchCapacityClaim::Draining),
        super::CapacityAdmission::Open { free, .. } => free,
    };
    let mut new_count = 0usize;
    let mut offers = Vec::with_capacity(request_ids.len());
    for request_id in request_ids {
        let subject = format!("m{message_id}r{request_id}");
        let existing = existing_offer(conn, &subject).await?;
        let state = if let Some(state) = existing {
            state
        } else if new_count < maximum_new && free > 0 {
            let launch_id = insert_offer(conn, &subject, message_id).await?;
            new_count = new_count.saturating_add(1);
            free -= 1;
            BatchOfferState::Reserved { launch_id }
        } else {
            BatchOfferState::Deferred
        };
        offers.push(BatchOfferClaim {
            request_id: *request_id,
            state,
        });
    }
    Ok(BatchCapacityClaim::Offers(offers))
}

async fn existing_offer(
    conn: &turso::Connection,
    subject: &str,
) -> Result<Option<BatchOfferState>, HostError> {
    let mut rows = conn
        .query(
            "SELECT id, state, replay_key_version, effect_state, docker_id, github_runner_id, dind_id, worker_volume, runner_name, observed_job_id, observed_workflow_run_id, cleanup_proven, remote_terminal FROM intents WHERE kind = 'launch' AND subject = ?1 AND NOT (state = 'failed' AND replay_key_version = 1 AND effect_state = 'definite_no_effect' AND docker_id IS NULL AND github_runner_id IS NULL AND dind_id IS NULL AND worker_volume IS NULL AND observed_job_id IS NULL AND observed_workflow_run_id IS NULL AND remote_terminal = 0) ORDER BY id DESC LIMIT 1",
            [subject],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let launch_id = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
    let state = row.get::<String>(1).map_err(|_| HostError::Journal)?;
    let replay_version = row.get::<i64>(2).map_err(|_| HostError::Journal)?;
    let effect_state = row.get::<String>(3).map_err(|_| HostError::Journal)?;
    let docker = row
        .get::<Option<String>>(4)
        .map_err(|_| HostError::Journal)?;
    let github_runner = row
        .get::<Option<String>>(5)
        .map_err(|_| HostError::Journal)?;
    let dind = row
        .get::<Option<String>>(6)
        .map_err(|_| HostError::Journal)?;
    let volume = row
        .get::<Option<String>>(7)
        .map_err(|_| HostError::Journal)?;
    let runner_name = row
        .get::<Option<String>>(8)
        .map_err(|_| HostError::Journal)?;
    let observed_job = row
        .get::<Option<String>>(9)
        .map_err(|_| HostError::Journal)?;
    let observed_run = row.get::<Option<i64>>(10).map_err(|_| HostError::Journal)?;
    let cleanup = row.get::<i64>(11).map_err(|_| HostError::Journal)?;
    let terminal = row.get::<i64>(12).map_err(|_| HostError::Journal)?;
    let state = if replay_version == 1
        && effect_state == "not_started"
        && state == "pending"
        && docker.is_none()
        && github_runner.is_none()
        && dind.is_none()
        && volume.is_none()
        && runner_name.is_none()
        && observed_job.is_none()
        && observed_run.is_none()
        && cleanup == 0
        && terminal == 0
    {
        BatchOfferState::Ready { launch_id }
    } else {
        BatchOfferState::Existing { launch_id }
    };
    Ok(Some(state))
}

async fn insert_offer(
    conn: &turso::Connection,
    subject: &str,
    message_id: i64,
) -> Result<i64, HostError> {
    conn.execute(
        "INSERT INTO intents (kind, subject, state, replay_key_version, effect_state, runner_start_state, message_id) VALUES ('launch', ?1, 'pending', 1, 'not_started', 'not_requested', ?2)",
        (subject, message_id),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(conn.last_insert_rowid())
}
