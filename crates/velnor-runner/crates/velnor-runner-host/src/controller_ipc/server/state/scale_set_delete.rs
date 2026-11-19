// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::error::HostError;
use crate::scale_set::delete::ScaleSetDeleteOutcome;

use super::super::super::wire::ScaleSetDeleteStatus;
use super::ControlState;
use super::session_observation::valid_nonce;

const TICKET_TTL: Duration = Duration::from_secs(600);

#[derive(Debug, Default)]
pub(super) struct ScaleSetDeleteTickets {
    tickets: BTreeMap<String, DeleteTicket>,
}

#[derive(Debug)]
struct DeleteTicket {
    nonce: String,
    status: ScaleSetDeleteStatus,
    expires_at: Instant,
}

impl ControlState {
    pub(in super::super) fn queue_scale_set_delete(
        &self,
        connection_id: &str,
        nonce: &str,
    ) -> Result<ScaleSetDeleteStatus, HostError> {
        if !valid_nonce(nonce) {
            return Err(HostError::Frame);
        }
        let now = Instant::now();
        let mut deletes = self
            .scale_set_deletes
            .lock()
            .map_err(|_| HostError::Journal)?;
        deletes.tickets.retain(|_, ticket| ticket.expires_at > now);
        if let Some(ticket) = deletes.tickets.get(connection_id)
            && ticket.nonce == nonce
        {
            return Ok(ticket.status);
        }
        let expires_at = now.checked_add(TICKET_TTL).ok_or(HostError::Identity)?;
        deletes.tickets.insert(
            connection_id.to_owned(),
            DeleteTicket {
                nonce: nonce.to_owned(),
                status: ScaleSetDeleteStatus::Pending,
                expires_at,
            },
        );
        Ok(ScaleSetDeleteStatus::Pending)
    }

    pub(in super::super) fn scale_set_delete_status(
        &self,
        connection_id: &str,
        nonce: &str,
    ) -> Result<ScaleSetDeleteStatus, HostError> {
        let deletes = self
            .scale_set_deletes
            .lock()
            .map_err(|_| HostError::Journal)?;
        Ok(deletes
            .tickets
            .get(connection_id)
            .filter(|ticket| ticket.nonce == nonce && ticket.expires_at > Instant::now())
            .map_or(ScaleSetDeleteStatus::Stale, |ticket| ticket.status))
    }

    pub(crate) fn pending_scale_set_delete(
        &self,
        connection_id: &str,
    ) -> Result<Option<String>, HostError> {
        let deletes = self
            .scale_set_deletes
            .lock()
            .map_err(|_| HostError::Journal)?;
        Ok(deletes
            .tickets
            .get(connection_id)
            .filter(|ticket| {
                ticket.status == ScaleSetDeleteStatus::Pending && ticket.expires_at > Instant::now()
            })
            .map(|ticket| ticket.nonce.clone()))
    }

    pub(crate) fn finish_scale_set_delete(
        &self,
        connection_id: &str,
        nonce: &str,
        outcome: ScaleSetDeleteOutcome,
    ) -> Result<(), HostError> {
        let mut deletes = self
            .scale_set_deletes
            .lock()
            .map_err(|_| HostError::Journal)?;
        if let Some(ticket) = deletes.tickets.get_mut(connection_id)
            && ticket.nonce == nonce
        {
            ticket.status = match outcome {
                ScaleSetDeleteOutcome::Deleted => ScaleSetDeleteStatus::Deleted,
                ScaleSetDeleteOutcome::NotOwned => ScaleSetDeleteStatus::NotOwned,
                ScaleSetDeleteOutcome::NotDrained => ScaleSetDeleteStatus::Stale,
                ScaleSetDeleteOutcome::Refused(status) => ScaleSetDeleteStatus::Refused { status },
                ScaleSetDeleteOutcome::Uncertain => ScaleSetDeleteStatus::Uncertain,
            };
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
