//! One-shot close of the exact session created by a verified pool capability.

use zeroize::Zeroize;

use crate::DiscoveryTransport;
use crate::session::{SessionError, delete_session};

use super::VerifiedPoolSessionAdmin;
use super::origin::bind_admin_origin;
use super::types::{SessionCloseOutcome, VerifiedQueueSession};

impl VerifiedPoolSessionAdmin {
    /// Close only this capability's exact session. The caller must first prove
    /// that all locally owned worker processes and containers have terminated;
    /// this protocol wrapper cannot observe Docker or journal state.
    ///
    /// The method returns `Held` without sending DELETE when a message, offer,
    /// acquired request, assigned-demand permit, or uncertain JIT is still
    /// outstanding. It binds the exact admin origin before DELETE. A dispatched
    /// DELETE is one-shot; credentials are retired after every dispatched outcome.
    ///
    /// # Errors
    ///
    /// Returns a registration error when the capability does not own this
    /// exact session, or the secret-safe service/transport error from the one
    /// DELETE attempt.
    pub fn close_session<T>(
        &mut self,
        transport: &mut T,
        session: &mut VerifiedQueueSession,
    ) -> Result<SessionCloseOutcome, SessionError>
    where
        T: DiscoveryTransport + ?Sized,
    {
        if self.close_attempted
            || self.created_session_id.as_deref() != Some(session.inner.session_id.as_str())
            || session.one_shot.close_attempted
            || session.scale_set_id != self.binding.scale_set_id
            || session.policy_digest != self.policy_digest
            || session.inner.session_id.is_empty()
        {
            return Err(SessionError::Wire(crate::WireError::RegistrationRejected));
        }
        if session.last_batch.is_some()
            || session.unresolved_available
            || !session.available_requests.is_empty()
            || !session.unresolved_requests.is_empty()
            || !session.acquired_requests.is_empty()
            || session.active_assigned_demand.is_some()
            || session.one_shot.assigned_jit_uncertain
        {
            return Ok(SessionCloseOutcome::Held);
        }

        self.require_session(session)?;
        bind_admin_origin(transport, self)?;

        self.close_attempted = true;
        session.one_shot.close_attempted = true;
        let result = delete_session(
            transport,
            self.binding.scale_set_id,
            &session.inner.session_id,
            self.connection.expose_token(),
        );

        // The exact close was dispatched and cannot be retried through this
        // capability, even if its response was lost or rejected.
        self.connection.retire();
        session.inner.retire();
        session.queue_path.zeroize();
        session.population_observation = None;
        session.last_batch = None;
        session.available_requests.clear();
        session.unresolved_requests.clear();
        session.acquired_requests.clear();
        match result {
            Ok(()) => Ok(SessionCloseOutcome::Closed),
            Err(error) => Err(error),
        }
    }
}
