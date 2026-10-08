//! Explicit local disposition for trusted offers left out of Acquire.

use crate::policy::VerifiedJobTrust;
use crate::{SessionError, WireError};

use super::VerifiedPoolSessionAdmin;
use super::types::VerifiedQueueSession;

impl VerifiedPoolSessionAdmin {
    /// Mark this exact verified offer as intentionally left out of Acquire for
    /// the current batch. This is a local disposition only: no upstream defer
    /// operation exists. The protocol leaves unrequested offers unassigned;
    /// its pinned README documents up to three cancel/requeue cycles with
    /// incremental delays for jobs not acquired in time. That is a retry-count
    /// bound, not a delay bound or a stable request-ID guarantee. An N+1 wave
    /// still needs a real end-to-end exercise; this local disposition cannot
    /// claim bounded progress.
    ///
    /// The caller must persist its chosen capacity slot and this unrequested
    /// disposition before calling, and must only use this for an ID that was
    /// never submitted to Acquire. Any ID submitted to Acquire is removed from
    /// `available_requests` before dispatch and remains unresolved on omitted,
    /// Noop, or transport-error outcomes; this method cannot clear it.
    ///
    /// # Errors
    ///
    /// Returns a registration error if the trust token belongs to another
    /// batch/session/policy, the offer is not still unrequested, or ACK was
    /// already attempted.
    pub fn leave_unrequested_available(
        &self,
        session: &mut VerifiedQueueSession,
        trust: &VerifiedJobTrust,
    ) -> Result<(), SessionError> {
        self.require_fresh()?;
        self.require_session(session)?;
        if session.one_shot.ack_attempted {
            return Err(SessionError::Wire(WireError::RegistrationRejected));
        }

        let (_, request_id) = self.verify_acquire_identity(session, trust)?;
        if !session.unrequested_requests.insert(request_id) {
            return Err(SessionError::Wire(WireError::RegistrationRejected));
        }
        if !session.available_requests.remove(&request_id) {
            session.unrequested_requests.remove(&request_id);
            return Err(SessionError::Wire(WireError::RegistrationRejected));
        }
        // Statistics describe the service's whole assigned population, which
        // still includes this deliberately deferred request. Do not let the
        // same observation mint a generic-assigned JIT for that demand.
        session.population_observation = None;
        Ok(())
    }
}
