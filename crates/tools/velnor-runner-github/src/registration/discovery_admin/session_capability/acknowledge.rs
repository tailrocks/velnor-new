use std::collections::BTreeSet;

use zeroize::Zeroize;

use crate::refresh::RefreshGate;
use crate::session::{Ack, AckScope, SessionError, SessionRequest, ack};
use crate::{DiscoveryTransport, ParsedBatch, WireError, may_ack};

use super::VerifiedPoolSessionAdmin;
use super::origin::{bind_admin_origin, bind_queue_origin};
use super::types::VerifiedQueueSession;
use super::valid_queue_path;

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
    pub fn acknowledge_resolved_message<T, F>(
        &self,
        transport: &mut T,
        session: &mut VerifiedQueueSession,
        message_id: i64,
        replay_safe: bool,
        gate: &RefreshGate,
        mut route_after_refresh: F,
    ) -> Result<Ack, SessionError>
    where
        T: DiscoveryTransport + ?Sized,
        F: FnMut(&mut T, &str, &mut SessionRequest) -> Result<String, SessionError>,
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
            replay_safe,
            session.unresolved_available,
            &session.available_requests,
            &session.unresolved_requests,
            &session.acquired_requests,
        ) {
            return Ok(Ack::Suppressed);
        }

        bind_queue_origin(transport, session)?;
        session.one_shot.ack_attempted = true;
        session.population_observation = None;
        let queue_token = session.inner.token().to_owned();
        let queue_path = session.queue_path.clone();
        let result = ack(
            transport,
            &queue_path,
            &batch,
            &AckScope {
                replay_safe,
                sole_unacquired_offer: false,
                queue_token: &queue_token,
            },
            gate,
            |transport, request| {
                refresh_ack_and_route(
                    transport,
                    self,
                    session,
                    request,
                    message_id,
                    &mut route_after_refresh,
                )
            },
        );
        let mut queue_token = queue_token;
        queue_token.zeroize();
        match result? {
            Ack::Deleted => {
                session.last_batch = None;
                session.last_message_id = None;
                Ok(Ack::Deleted)
            }
            Ack::Suppressed => Ok(Ack::Suppressed),
        }
    }
}

fn refresh_ack_and_route<T, F>(
    transport: &mut T,
    capability: &VerifiedPoolSessionAdmin,
    session: &mut VerifiedQueueSession,
    request: &mut SessionRequest,
    message_id: i64,
    route_after_refresh: &mut F,
) -> Result<(), SessionError>
where
    T: DiscoveryTransport + ?Sized,
    F: FnMut(&mut T, &str, &mut SessionRequest) -> Result<String, SessionError>,
{
    bind_admin_origin(transport, capability)?;
    let queue_url = crate::session::refresh_queue_request(
        transport,
        capability.binding.scale_set_id,
        &mut session.inner,
        capability.connection.expose_token(),
        request,
    )?;
    let refreshed_base = route_after_refresh(transport, queue_url, request)?;
    if !valid_queue_path(&refreshed_base) {
        return Err(SessionError::Wire(WireError::RegistrationRejected));
    }

    session.queue_path.clone_from(&refreshed_base);
    request.path = format!("{}/{message_id}", refreshed_base.trim_end_matches('/'));
    Ok(())
}

fn acknowledgement_allowed(
    batch: &ParsedBatch,
    replay_safe: bool,
    unresolved_available: bool,
    available_requests: &BTreeSet<i64>,
    unresolved_requests: &BTreeSet<i64>,
    acquired_requests: &BTreeSet<i64>,
) -> bool {
    !unresolved_available
        && available_requests.is_empty()
        && unresolved_requests.is_empty()
        && acquired_requests.is_empty()
        && may_ack(batch, replay_safe)
}

#[cfg(test)]
mod tests;
