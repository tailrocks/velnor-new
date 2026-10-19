//! Database records for one capacity reservation and its `AcquireJobs` permission.

use crate::error::HostError;
use crate::journal::connection::AssignmentIdentity;
use crate::journal::connection::storage::in_transaction;
use crate::journal::request_lifecycle::RequestAcquireState;
use crate::launch::StartPermit;

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
        reservation_id: i64,
        intent_id: i64,
        acquire_state: RequestAcquireState,
    },
    /// Current host policy permits no additional capacity reservation of this size.
    AtCapacity,
    /// If the earlier capacity reservation has no request connection, reconcile it before acquisition.
    ReconcileRequired { reservation_id: i64, intent_id: i64 },
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
                ReservationResult::Reserved { reservation_id }
                | ReservationResult::Resumed { reservation_id } => reservation_id,
                ReservationResult::AtCapacity => {
                    return Ok(RequestPhysicalReservationResult::AtCapacity);
                }
                ReservationResult::ReconcileRequired { reservation_id }
                | ReservationResult::Released { reservation_id } => {
                    let intent_id = reservation_intent_id(connection, reservation_id).await?;
                    return Ok(RequestPhysicalReservationResult::ReconcileRequired {
                        reservation_id,
                        intent_id,
                    });
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
            Ok(RequestPhysicalReservationResult::AcquireClaimed {
                reservation_id,
                intent_id,
            })
        })
        .await
    }
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
            "SELECT link.reservation_id, reservation.intent_id, link.physical_nonce, link.binding_digest, reservation.connection_id, reservation.assignment_key, reservation.engine_id, reservation.docker_root_digest, reservation.cpu_millicores, reservation.memory_bytes, intent.kind, intent.scale_set_id, intent.runner_name, intent.runner_request_id, auth.acquire_state FROM connection_request_reservations AS link JOIN capacity_reservations AS reservation ON reservation.id = link.reservation_id JOIN connection_intents AS intent ON intent.id = reservation.intent_id JOIN connection_request_authorizations AS auth ON auth.connection_id = link.connection_id AND auth.scale_set_id = link.scale_set_id AND auth.request_id = link.request_id WHERE link.connection_id = ?1 AND link.scale_set_id = ?2 AND link.request_id = ?3",
            (
                scope.connection_id().as_str(),
                request.scale_set_id(),
                request.request_id().ok_or(HostError::Identity)?,
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
        || intent_request_id.is_some()
    {
        return Err(HostError::Journal);
    }
    let acquire_state = crate::journal::request_lifecycle::acquire::parse_state(&acquire_state)?;
    Ok(Some(RequestPhysicalReservationResult::Existing {
        reservation_id,
        intent_id,
        acquire_state,
    }))
}

async fn reservation_intent_id(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<i64, HostError> {
    let mut rows = connection
        .query(
            "SELECT intent_id FROM capacity_reservations WHERE id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let intent_id = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if intent_id <= 0 {
        return Err(HostError::Journal);
    }
    Ok(intent_id)
}

fn physical_nonce(key: &crate::journal::connection::AssignmentKey) -> Result<&str, HostError> {
    let crate::journal::connection::AssignmentKey::Remote { kind, subject } = key else {
        return Err(HostError::Identity);
    };
    if kind != "runner" || !lower_hex(subject, 32) {
        return Err(HostError::Identity);
    }
    Ok(subject)
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
