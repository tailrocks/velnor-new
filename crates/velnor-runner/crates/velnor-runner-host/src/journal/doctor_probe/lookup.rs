//! Read-only lookup for the one durable active Doctor request.

use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::connection::storage::read_transaction;

use super::DoctorProbeRequestRecord;
use super::api::{active_request_id, record, ticket_by_id};

impl Journal {
    /// Find the queued or running Doctor probe request so the host can start its work again after restart.
    ///
    /// This method reads records only. The journal connects a queued request to its probe and lease
    /// in `prepare_probe_for_doctor_request` before the controller starts Docker work.
    pub(crate) async fn active_doctor_probe_request(
        &self,
    ) -> Result<Option<DoctorProbeRequestRecord>, HostError> {
        read_transaction(self, async |connection| {
            let Some(request_id) = active_request_id(connection).await? else {
                return Ok(None);
            };
            let ticket = ticket_by_id(connection, request_id).await?;
            Ok(Some(record(connection, ticket).await?))
        })
        .await
    }
}
