//! All-or-none reservations for one bounded queue-offer batch.

use std::collections::BTreeSet;
use std::num::NonZeroU32;

use super::batch::{existing_offer, insert_offer};
use super::{BatchOfferClaim, BatchOfferState, CapacityAdmission, Journal};
use crate::error::HostError;

/// Atomic outcome when reserving every fresh request in one offered batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtomicBatchCapacityClaim {
    /// Every request is already represented or was reserved in this transaction.
    /// Unlike the partial API, an `Offers` result never contains `Deferred`.
    Offers(Vec<BatchOfferClaim>),
    /// Drain was committed before this batch could add any new reservation.
    Draining,
    /// The journal held fewer permits than the number of fresh requests.
    CapacityFull {
        /// All unresolved launch rows across routes and scopes.
        occupied: u64,
        /// Fresh requests that this transaction would need to reserve.
        required: u64,
        /// Permits available before this transaction.
        available: u64,
        /// Configured host-wide maximum.
        maximum: NonZeroU32,
    },
}

impl Journal {
    /// Atomically reserve every fresh request in one bounded queue batch.
    ///
    /// Existing exact offer rows are replayed in input order. If any new row
    /// cannot fit under the shared host-wide capacity gate, this call returns
    /// `CapacityFull` without inserting any of the new rows. The existing
    /// partial-reservation API remains unchanged. Request IDs use that API's
    /// exact `m{message_id}r{request_id}` replay subjects.
    ///
    /// The result records reservations only; it does not authorize an
    /// `AcquireJobs`, runner, JIT, or Docker effect. Every effect still requires
    /// its ordinary durable identity binding and effect-intent transition.
    /// Callers must replay the exact batch after an ambiguous commit error.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for malformed/duplicate IDs, batches
    /// larger than 50, read-only use, or transaction/database failure.
    pub async fn reserve_launch_batch_all_or_none_if_accepting(
        &self,
        message_id: i64,
        request_ids: &[i64],
        maximum: NonZeroU32,
    ) -> Result<AtomicBatchCapacityClaim, HostError> {
        if self.read_only
            || message_id < 0
            || request_ids.is_empty()
            || request_ids.len() > 50
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
        let result = reserve_batch_in_transaction(&conn, message_id, request_ids, maximum).await;
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
    maximum: NonZeroU32,
) -> Result<AtomicBatchCapacityClaim, HostError> {
    let mut claims = vec![None; request_ids.len()];
    let mut missing = Vec::new();
    for (index, request_id) in request_ids.iter().enumerate() {
        let subject = format!("m{message_id}r{request_id}");
        match existing_offer(conn, &subject).await? {
            Some(BatchOfferState::Deferred) => return Err(HostError::Journal),
            Some(state) => {
                claims[index] = Some(BatchOfferClaim {
                    request_id: *request_id,
                    state,
                });
            }
            None => missing.push(index),
        }
    }

    if missing.is_empty() {
        return Ok(AtomicBatchCapacityClaim::Offers(
            claims.into_iter().flatten().collect(),
        ));
    }

    let admission = match super::capacity_admission(conn, maximum).await? {
        CapacityAdmission::Draining => return Ok(AtomicBatchCapacityClaim::Draining),
        CapacityAdmission::Open { occupied, free } => (occupied, free),
    };
    let (occupied, available) = admission;
    let required = u64::try_from(missing.len()).map_err(|_| HostError::Journal)?;
    if available < required {
        return Ok(AtomicBatchCapacityClaim::CapacityFull {
            occupied,
            required,
            available,
            maximum,
        });
    }

    for index in missing {
        let request_id = request_ids[index];
        let subject = format!("m{message_id}r{request_id}");
        let launch_id = insert_offer(conn, &subject, message_id).await?;
        claims[index] = Some(BatchOfferClaim {
            request_id,
            state: BatchOfferState::Reserved { launch_id },
        });
    }
    claims
        .into_iter()
        .map(|claim| claim.ok_or(HostError::Journal))
        .collect::<Result<Vec<_>, _>>()
        .map(AtomicBatchCapacityClaim::Offers)
}
