// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::connection::storage::read_transaction;

use super::DoctorProbeRequestRecord;
use super::api::{active_request_id, record, ticket_by_id};

impl Journal {
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
