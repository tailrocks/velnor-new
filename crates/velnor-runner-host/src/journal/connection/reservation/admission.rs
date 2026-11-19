// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use crate::launch::StartPermit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReservationRequest<'a> {
    pub(crate) assignment: &'a super::super::AssignmentIdentity,

    pub(crate) engine_id: &'a str,

    pub(crate) docker_root_digest: &'a str,

    pub(crate) cpu_millicores: u64,

    pub(crate) memory_bytes: u64,

    pub(crate) sample_generation_id: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReservationResult {
    Reserved { reservation_id: i64 },

    ReconcileRequired { reservation_id: i64 },

    Released { reservation_id: i64 },

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
