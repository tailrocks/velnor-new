use std::collections::BTreeSet;

use zeroize::Zeroize;

use crate::refresh::RefreshGate;
use crate::session::{Ack, AckScope, SessionError, ack_with_route};
use crate::{DiscoveryTransport, InnerKind, ParsedBatch, SessionRequest, WireError, may_ack};

use super::VerifiedPoolSessionAdmin;
use super::origin::{bind_admin_origin, bind_queue_origin};
use super::types::VerifiedQueueSession;

impl VerifiedPoolSessionAdmin {
    /// Acknowledge only the exact latest message, after every unique Available
    /// request it carried has completed Acquire and JIT. `replay_safe` must
    /// come from the State journal and cover any non-Available events.
    /// Unknown/malformed Available offers, omitted Acquire IDs, acquired jobs
    /// without successful JIT, and any already-attempted ACK keep the message
    /// held. The ACK attempt is one-shot because a transport error is uncertain.
    ///
    /// This method never closes the session. Callers retain the session on a
    /// suppressed or uncertain result and must not poll past an unacknowledged
    /// batch through this capability.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe session or transport error. A transport error
    /// after the DELETE is sent is uncertain and this capability will reject a
    /// second ACK attempt for the same message.
    pub fn acknowledge_resolved_message<T>(
        &self,
        transport: &mut T,
        session: &mut VerifiedQueueSession,
        message_id: i64,
        replay_safe: bool,
        gate: &RefreshGate,
    ) -> Result<Ack, SessionError>
    where
        T: DiscoveryTransport + ?Sized,
    {
        self.require_session(session)?;
        if session.last_message_id != Some(message_id)
            || session.one_shot.ack_attempted
            || session.active_assigned_demand.is_some()
            || session
                .last_batch
                .as_ref()
                .is_none_or(|batch| batch.message_id != message_id)
        {
            return Err(SessionError::Wire(WireError::RegistrationRejected));
        }
        let Some(batch) = session.last_batch.clone() else {
            return Err(SessionError::Wire(WireError::RegistrationRejected));
        };
        if !acknowledgement_allowed(
            &batch,
            AckState {
                replay_safe,
                unresolved_available: session.unresolved_available,
                available_requests: &session.available_requests,
                unrequested_requests: &session.unrequested_requests,
                unresolved_requests: &session.unresolved_requests,
                acquired_requests: &session.acquired_requests,
                completed_requests: &session.completed_requests,
            },
        ) {
            return Ok(Ack::Suppressed);
        }

        bind_queue_origin(transport, session)?;
        session.one_shot.ack_attempted = true;
        session.population_observation = None;
        let queue_token = session.inner.token().to_owned();
        let queue_route = session.queue_route.duplicate();
        let result = ack_with_route(
            transport,
            &queue_route,
            &batch,
            &AckScope {
                replay_safe,
                sole_unacquired_offer: false,
                queue_token: &queue_token,
            },
            gate,
            |transport, request| {
                refresh_ack_and_route(transport, self, session, request, message_id)
            },
        );
        let mut queue_token = queue_token;
        queue_token.zeroize();
        match result? {
            Ack::Deleted => {
                session.last_batch = None;
                session.last_message_id = None;
                session.unrequested_requests.clear();
                session.completed_requests.clear();
                Ok(Ack::Deleted)
            }
            Ack::Suppressed => Ok(Ack::Suppressed),
        }
    }
}

fn refresh_ack_and_route<T>(
    transport: &mut T,
    capability: &VerifiedPoolSessionAdmin,
    session: &mut VerifiedQueueSession,
    request: &mut SessionRequest,
    message_id: i64,
) -> Result<(), SessionError>
where
    T: DiscoveryTransport + ?Sized,
{
    bind_admin_origin(transport, capability)?;
    let queue_url = crate::session::refresh_queue_request(
        transport,
        capability.binding.scale_set_id,
        &mut session.inner,
        capability.connection.expose_token(),
        request,
    )?;
    let refreshed_route = transport.bind_message_queue_origin(queue_url)?;
    let path = refreshed_route.acknowledgement_path(message_id);
    let query = refreshed_route.query().map(str::to_owned);
    request.replace_target(path, query);
    session.queue_route.replace_with(refreshed_route);
    Ok(())
}

#[derive(Clone, Copy)]
struct AckState<'a> {
    replay_safe: bool,
    unresolved_available: bool,
    available_requests: &'a BTreeSet<i64>,
    unrequested_requests: &'a BTreeSet<i64>,
    unresolved_requests: &'a BTreeSet<i64>,
    acquired_requests: &'a BTreeSet<i64>,
    completed_requests: &'a BTreeSet<i64>,
}

fn acknowledgement_allowed(batch: &ParsedBatch, state: AckState<'_>) -> bool {
    !state.unresolved_available
        && state.available_requests.is_empty()
        && state
            .unrequested_requests
            .is_disjoint(state.available_requests)
        && state
            .unrequested_requests
            .is_disjoint(state.unresolved_requests)
        && state
            .unrequested_requests
            .is_disjoint(state.acquired_requests)
        && state
            .unrequested_requests
            .is_disjoint(state.completed_requests)
        && state.unresolved_requests.is_empty()
        && state.acquired_requests.is_empty()
        && offers_accounted_for(batch, state.unrequested_requests, state.completed_requests)
        && may_ack(batch, state.replay_safe)
}

fn offers_accounted_for(
    batch: &ParsedBatch,
    unrequested_requests: &BTreeSet<i64>,
    completed_requests: &BTreeSet<i64>,
) -> bool {
    if !unrequested_requests.is_disjoint(completed_requests) {
        return false;
    }

    let mut available_ids = BTreeSet::new();
    let mut available_count = 0usize;
    for job in &batch.jobs {
        if !matches!(job.kind, InnerKind::Available) {
            continue;
        }
        available_count += 1;
        let Some(request_id) = job.request_id.filter(|id| *id > 0) else {
            return false;
        };
        if !available_ids.insert(request_id) {
            return false;
        }
    }

    if available_ids.len() != available_count
        || available_ids.len() != unrequested_requests.len() + completed_requests.len()
    {
        return false;
    }
    available_ids
        .iter()
        .all(|id| unrequested_requests.contains(id) || completed_requests.contains(id))
}

#[cfg(test)]
mod tests;
