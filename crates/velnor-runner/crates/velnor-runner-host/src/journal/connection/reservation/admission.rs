//! Atomic reservation admission before any remote or Docker effect.

use crate::error::HostError;
use crate::launch::StartPermit;

#[cfg(test)]
mod generic;
#[cfg(test)]
mod record;

/// Resources charged by one assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReservationRequest<'a> {
    /// Typed connection and Scale Set assignment identity.
    pub(crate) assignment: &'a super::super::AssignmentIdentity,
    /// Selected Docker engine identity.
    pub(crate) engine_id: &'a str,
    /// Digest of the selected Docker root.
    pub(crate) docker_root_digest: &'a str,
    /// Total runner and private `DinD` CPU charge.
    pub(crate) cpu_millicores: u64,
    /// Total runner and private `DinD` memory charge.
    pub(crate) memory_bytes: u64,
    /// Resource sample generation that authorized this charge.
    pub(crate) sample_generation_id: &'a str,
}

/// Result of a durable reservation attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReservationResult {
    /// A live reservation exists.
    Reserved {
        /// Durable reservation identity.
        reservation_id: i64,
    },
    /// Existing capacity was revalidated with a fresh, single-use sample.
    #[cfg(test)]
    Resumed {
        /// Durable reservation identity.
        reservation_id: i64,
    },
    /// Existing effects or uncertainty require reconciliation before resuming.
    ReconcileRequired {
        /// Durable reservation identity.
        reservation_id: i64,
    },
    /// Exact cleanup for this assignment has ended.
    Released {
        /// Durable reservation identity.
        reservation_id: i64,
    },
    /// The shared host budget cannot accept this charge.
    AtCapacity,
}

pub(super) fn validate_permit(
    request: ReservationRequest<'_>,
    permit: &StartPermit,
) -> Result<(), HostError> {
    if permit.engine_id() != request.engine_id
        || permit.docker_root_digest() != request.docker_root_digest
        || permit.charge() != (request.cpu_millicores, request.memory_bytes)
        || permit.sample_generation_id() != request.sample_generation_id
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) async fn ensure_sample_is_new(
    connection: &turso::Connection,
    generation_id: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM capacity_reservation_samples WHERE sample_generation_id = ?1 LIMIT 1",
            [generation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

pub(super) async fn record_sample_use(
    connection: &turso::Connection,
    reservation_id: i64,
    generation_id: &str,
    purpose: &str,
) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO capacity_reservation_samples (reservation_id, sample_generation_id, purpose) VALUES (?1, ?2, ?3)",
            (reservation_id, generation_id, purpose),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}
