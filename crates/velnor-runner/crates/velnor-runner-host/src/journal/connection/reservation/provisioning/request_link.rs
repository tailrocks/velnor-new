//! Database records for one capacity reservation and its `AcquireJobs` permission.

use crate::error::HostError;
use crate::journal::connection::AssignmentIdentity;
use crate::journal::connection::storage::in_transaction;
#[cfg(test)]
use crate::journal::request_lifecycle::RequestAcquireState;
use crate::launch::StartPermit;

mod successor;
mod verification;
use verification::{actual_request_matches_link, physical_nonce, reservation_intent_id};

use super::super::admission::{ReservationRequest, ReservationResult};
use super::super::storage_budget::StorageProvisioningAdmission;
use super::ConnectionJournal;
use super::admission::reserve_in_transaction;

/// Journal result that connects one acquired request intent to its capacity reservation.
pub(crate) enum RequestPhysicalReservationResult {
    /// One database transaction records the new capacity reservation and permission for one `AcquireJobs` call.
    AcquireClaimed { reservation_id: i64, intent_id: i64 },
    /// Return the recorded connection without use of another permit or sample.
    Existing {
        #[cfg(test)]
        reservation_id: i64,
        #[cfg(test)]
        intent_id: i64,
        #[cfg(test)]
        acquire_state: RequestAcquireState,
    },
    /// Current host policy permits no additional capacity reservation of this size.
    AtCapacity,
    /// If the earlier capacity reservation has no request connection, reconcile it before acquisition.
    ReconcileRequired {
        #[cfg(test)]
        reservation_id: i64,
        #[cfg(test)]
        intent_id: i64,
    },
}

/// This result describes a new physical reservation for a request that was acquired earlier.
pub(crate) enum PhysicalSuccessorReservationResult {
    /// The transaction committed the new reservation and physical attempt. It did not send an `AcquireJobs` request.
    Reserved { reservation_id: i64 },
    /// The current shared resource policy rejected the new physical attempt.
    AtCapacity,
    /// Existing physical effects for this assignment need reconciliation.
    ReconcileRequired,
}

impl ConnectionJournal {
    /// Record one runner capacity reservation and its `AcquireJobs` permission in one database transaction.
    pub(crate) async fn try_reserve_and_claim_request_acquire(
        &self,
        physical: ReservationRequest<'_>,
        permit: StartPermit,
        admission: StorageProvisioningAdmission,
        request: &AssignmentIdentity,
    ) -> Result<RequestPhysicalReservationResult, HostError> {
        let nonce = physical_nonce(physical.assignment.key())?.to_owned();
        let request_id = request.request_id().ok_or(HostError::Identity)?;
        if request.connection_id() != self.connection_id()
            || physical.assignment.connection_id() != self.connection_id()
            || request.scale_set_id() != physical.assignment.scale_set_id()
            || request_id <= 0
        {
            return Err(HostError::Identity);
        }
        in_transaction(self.journal_ref(), async move |connection| {
            self.validate_scope(connection).await?;
            if let Some(existing) =
                existing_request_link(connection, self, physical, request, &nonce).await?
            {
                return Ok(existing);
            }

            let mut permit = Some(permit);
            let reservation = reserve_in_transaction(
                connection,
                self,
                physical,
                &mut permit,
                &admission,
            )
            .await?;
            let reservation_id = match reservation {
                ReservationResult::Reserved { reservation_id } => reservation_id,
                #[cfg(test)]
                ReservationResult::Resumed { reservation_id } => reservation_id,
                ReservationResult::AtCapacity => {
                    return Ok(RequestPhysicalReservationResult::AtCapacity);
                }
                ReservationResult::ReconcileRequired { reservation_id }
                | ReservationResult::Released { reservation_id } => {
                    return request_reconcile_required(connection, reservation_id).await;
                }
            };
            let intent_id = reservation_intent_id(connection, reservation_id).await?;
            if !crate::journal::request_lifecycle::acquire::claim_request_acquire_in_transaction(
                connection, self, request,
            )
            .await?
            {
                return Err(HostError::Journal);
            }
            connection
                .execute(
                    "INSERT INTO connection_request_reservations (connection_id, scale_set_id, request_id, reservation_id, physical_nonce, binding_digest) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (
                        self.connection_id().as_str(),
                        request.scale_set_id(),
                        request_id,
                        reservation_id,
                        nonce.as_str(),
                        self.binding_digest(),
                    ),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            super::super::physical_attempt::record_in_transaction(
                connection,
                self,
                reservation_id,
                intent_id,
                request_id,
                &nonce,
            )
            .await?;
            Ok(RequestPhysicalReservationResult::AcquireClaimed {
                reservation_id,
                intent_id,
            })
        })
        .await
    }
}

async fn request_reconcile_required(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<RequestPhysicalReservationResult, HostError> {
    #[cfg(test)]
    let intent_id = reservation_intent_id(connection, reservation_id).await?;
    #[cfg(not(test))]
    reservation_intent_id(connection, reservation_id).await?;
    Ok(RequestPhysicalReservationResult::ReconcileRequired {
        #[cfg(test)]
        reservation_id,
        #[cfg(test)]
        intent_id,
    })
}

async fn existing_request_link(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    physical: ReservationRequest<'_>,
    request: &AssignmentIdentity,
    nonce: &str,
) -> Result<Option<RequestPhysicalReservationResult>, HostError> {
    let mut rows = connection
        .query(
            "SELECT attempt.reservation_id, reservation.intent_id, attempt.physical_nonce, attempt.binding_digest, reservation.connection_id, reservation.assignment_key, reservation.engine_id, reservation.docker_root_digest, reservation.cpu_millicores, reservation.memory_bytes, intent.kind, intent.scale_set_id, intent.runner_name, intent.runner_request_id, auth.acquire_state FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id AND attempt.intent_id = current.intent_id JOIN capacity_reservations AS reservation ON reservation.id = attempt.reservation_id JOIN connection_intents AS intent ON intent.id = reservation.intent_id AND intent.connection_id = reservation.connection_id JOIN connection_request_authorizations AS auth ON auth.connection_id = attempt.connection_id AND auth.scale_set_id = attempt.scale_set_id AND auth.request_id = attempt.witness_request_id WHERE attempt.connection_id = ?1 AND attempt.scale_set_id = ?2 AND attempt.witness_request_id = ?3 AND attempt.physical_nonce = ?4",
            (
                scope.connection_id().as_str(),
                request.scale_set_id(),
                request.request_id().ok_or(HostError::Identity)?,
                nonce,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let reservation_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let intent_id: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let stored_nonce: String = row.get(2).map_err(|_| HostError::Journal)?;
    let binding_digest: String = row.get(3).map_err(|_| HostError::Journal)?;
    let connection_id: String = row.get(4).map_err(|_| HostError::Journal)?;
    let assignment_key: String = row.get(5).map_err(|_| HostError::Journal)?;
    let engine_id: String = row.get(6).map_err(|_| HostError::Journal)?;
    let root_digest: String = row.get(7).map_err(|_| HostError::Journal)?;
    let cpu: i64 = row.get(8).map_err(|_| HostError::Journal)?;
    let memory: i64 = row.get(9).map_err(|_| HostError::Journal)?;
    let intent_kind: String = row.get(10).map_err(|_| HostError::Journal)?;
    let intent_scale_set: i64 = row.get(11).map_err(|_| HostError::Journal)?;
    let runner_name: Option<String> = row.get(12).map_err(|_| HostError::Journal)?;
    let intent_request_id: Option<i64> = row.get(13).map_err(|_| HostError::Journal)?;
    let acquire_state: String = row.get(14).map_err(|_| HostError::Journal)?;
    let witness_request_id = request.request_id().ok_or(HostError::Identity)?;
    let actual_request_matches = match intent_request_id {
        Some(actual_request_id) => {
            actual_request_matches_link(
                connection,
                scope,
                reservation_id,
                intent_id,
                request.scale_set_id(),
                witness_request_id,
                actual_request_id,
            )
            .await?
        }
        None => true,
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || reservation_id <= 0
        || intent_id <= 0
        || stored_nonce != nonce
        || binding_digest != scope.binding_digest()
        || connection_id != scope.connection_id().as_str()
        || assignment_key != physical.assignment.storage_key()
        || engine_id != physical.engine_id
        || root_digest != physical.docker_root_digest
        || cpu != i64::try_from(physical.cpu_millicores).map_err(|_| HostError::Journal)?
        || memory != i64::try_from(physical.memory_bytes).map_err(|_| HostError::Journal)?
        || intent_kind != "launch"
        || intent_scale_set != physical.assignment.scale_set_id()
        || runner_name.as_deref() != Some(format!("v{nonce}").as_str())
        || !actual_request_matches
    {
        return Err(HostError::Journal);
    }
    #[cfg(test)]
    let acquire_state = crate::journal::request_lifecycle::acquire::parse_state(&acquire_state)?;
    #[cfg(not(test))]
    crate::journal::request_lifecycle::acquire::parse_state(&acquire_state)?;
    Ok(Some(RequestPhysicalReservationResult::Existing {
        #[cfg(test)]
        reservation_id,
        #[cfg(test)]
        intent_id,
        #[cfg(test)]
        acquire_state,
    }))
}
