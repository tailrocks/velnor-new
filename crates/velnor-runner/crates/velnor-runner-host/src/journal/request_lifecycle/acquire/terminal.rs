use crate::error::HostError;
use crate::journal::connection::storage::read_transaction;
use crate::journal::connection::{AssignmentIdentity, ConnectionJournal};
use crate::journal::source_authorization::AssignedAcquireResult;

use super::{RequestAcquireState, digest, parse_state, request_id};

impl ConnectionJournal {
    /// Read a terminal no-acquire result for the exact linked reservation.
    pub(crate) async fn known_no_acquire_disposition(
        &self,
        identity: &AssignmentIdentity,
    ) -> Result<Option<(RequestAcquireState, bool)>, HostError> {
        let request_id = request_id(identity, self)?;
        let connection_id = self.connection_id().as_str().to_owned();
        let binding_digest = self.binding_digest().to_owned();
        read_transaction(self.journal_ref(), async |connection| {
            super::super::helpers::validate_scope_identity(self, connection, identity).await?;
            let mut rows = connection
                .query(
                    "SELECT auth.acquire_state, auth.acquire_response_json, auth.acquire_response_digest, attempt.reservation_id, attempt.physical_nonce, reservation.assignment_key, reservation.state, reservation.remote_cleanup_proven, reservation.local_cleanup_proven, intent.assignment_key, intent.runner_name, intent.cleanup_proven FROM connection_request_authorizations AS auth JOIN physical_attempts AS attempt ON attempt.connection_id = auth.connection_id AND attempt.scale_set_id = auth.scale_set_id AND attempt.witness_request_id = auth.request_id AND attempt.binding_digest = auth.binding_digest JOIN capacity_reservations AS reservation ON reservation.id = attempt.reservation_id AND reservation.connection_id = attempt.connection_id JOIN connection_intents AS intent ON intent.id = reservation.intent_id AND intent.connection_id = reservation.connection_id WHERE auth.connection_id = ?1 AND auth.scale_set_id = ?2 AND auth.request_id = ?3 AND auth.binding_digest = ?4 AND auth.acquire_state IN ('no_effect', 'rejected')",
                    (
                        connection_id.as_str(),
                        identity.scale_set_id(),
                        request_id,
                        binding_digest.as_str(),
                    ),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            let state = parse_state(&row.get::<String>(0).map_err(|_| HostError::Journal)?)?;
            let response_json: String = row.get(1).map_err(|_| HostError::Journal)?;
            let response_digest: String = row.get(2).map_err(|_| HostError::Journal)?;
            let reservation_id: i64 = row.get(3).map_err(|_| HostError::Journal)?;
            let physical_nonce: String = row.get(4).map_err(|_| HostError::Journal)?;
            let reservation_assignment: String = row.get(5).map_err(|_| HostError::Journal)?;
            let reservation_state: String = row.get(6).map_err(|_| HostError::Journal)?;
            let remote_cleanup: i64 = row.get(7).map_err(|_| HostError::Journal)?;
            let local_cleanup: i64 = row.get(8).map_err(|_| HostError::Journal)?;
            let intent_assignment: String = row.get(9).map_err(|_| HostError::Journal)?;
            let runner_name: String = row.get(10).map_err(|_| HostError::Journal)?;
            let intent_cleanup: i64 = row.get(11).map_err(|_| HostError::Journal)?;
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
                return Err(HostError::Journal);
            }

            let result = match state {
                RequestAcquireState::NoEffect => AssignedAcquireResult::NoEffect,
                RequestAcquireState::Rejected => AssignedAcquireResult::Rejected,
                _ => return Ok(None),
            };
            let (_, expected_response) = super::response_record(&result, request_id)?;
            if response_json != expected_response
                || response_digest != digest(response_json.as_bytes())
                || reservation_id <= 0
                || runner_name.strip_prefix('v') != Some(physical_nonce.as_str())
                || reservation_assignment != intent_assignment
            {
                return Err(HostError::Journal);
            }

            match reservation_state.as_str() {
                "released" if remote_cleanup == 1 && local_cleanup == 1 && intent_cleanup == 1 => {
                    Ok(Some((state, true)))
                }
                "reserved" if remote_cleanup == 0 && local_cleanup == 0 && intent_cleanup == 0 => {
                    Ok(Some((state, false)))
                }
                _ => Err(HostError::Journal),
            }
        })
        .await
    }
}
