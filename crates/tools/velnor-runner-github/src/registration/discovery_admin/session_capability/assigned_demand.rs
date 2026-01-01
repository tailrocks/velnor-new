//! Bound generic JIT permits for exact assigned-population observations.

use crate::DiscoveryTransport;
use crate::EncodedJit;
use crate::WireError;
use crate::session::{SessionError, jit, jit_request};

use super::VerifiedPoolSessionAdmin;
use super::origin::bind_admin_origin;
use super::types::{
    ActiveAssignedDemand, PopulationObservationSource, VerifiedAssignedDemand, VerifiedQueueSession,
};

impl VerifiedPoolSessionAdmin {
    /// Turn the latest exact-session population observation into a bounded
    /// generic-JIT permit. This is the assigned-population path: it does not
    /// acquire a `JobAvailable`, and the returned permit has no job/request/run
    /// identity. The verified pool policy must prove that all jobs eligible
    /// for this homogeneous Scale Set are trusted.
    ///
    /// The permit count is capped by both `totalAssignedJobs -
    /// totalRunningJobs` in the same response and `free_capacity`. Pass only
    /// capacity that this coordinator has durably reserved. An `Available`
    /// offer, unresolved effect, missing/stale statistics, inconsistent
    /// counters, or zero capacity yields `None` without a JIT call.
    ///
    /// # Errors
    ///
    /// Returns a registration error for a stale or mismatched session, and a
    /// malformed-response error if assigned/running statistics are invalid.
    pub fn take_assigned_demand(
        &self,
        session: &mut VerifiedQueueSession,
        free_capacity: u32,
    ) -> Result<Option<VerifiedAssignedDemand>, SessionError> {
        self.require_fresh()?;
        self.require_session(session)?;
        if free_capacity == 0
            || session.active_assigned_demand.is_some()
            || session.unresolved_available
            || !session.available_requests.is_empty()
            || !session.unresolved_requests.is_empty()
            || !session.acquired_requests.is_empty()
        {
            return Ok(None);
        }

        let Some(observation) = session.population_observation.as_ref() else {
            return Ok(None);
        };
        if observation.session_id() != session.inner.session_id
            || observation.scale_set_id() != self.binding.scale_set_id
            || observation
                .message_id()
                .is_some_and(|message_id| session.last_message_id != Some(message_id))
            || (observation.source() == PopulationObservationSource::PollBatch
                && observation.message_id().is_none())
        {
            return Err(SessionError::Wire(WireError::RegistrationRejected));
        }
        let statistics = observation.statistics();
        let assigned = statistics.total_assigned_jobs;
        let running = statistics.total_running_jobs;
        if assigned < 0 || running < 0 || assigned < running {
            return Err(SessionError::Wire(WireError::Malformed));
        }
        let count = (assigned - running).min(i64::from(free_capacity));
        if count <= 0 {
            return Ok(None);
        }

        let demand_id = session.next_assigned_demand_id;
        let next_id = demand_id
            .checked_add(1)
            .ok_or(SessionError::Wire(WireError::RegistrationRejected))?;
        let count = u32::try_from(count).map_err(|_| SessionError::Wire(WireError::Malformed))?;
        let token = VerifiedAssignedDemand {
            demand_id,
            session_id: session.inner.session_id.clone(),
            scale_set_id: self.binding.scale_set_id,
            policy_digest: self.policy_digest.clone(),
            message_id: observation.message_id(),
            source: observation.source(),
            observed_at: observation.observed_at(),
            assigned_jobs: u32::try_from(assigned)
                .map_err(|_| SessionError::Wire(WireError::Malformed))?,
            running_jobs: u32::try_from(running)
                .map_err(|_| SessionError::Wire(WireError::Malformed))?,
            remaining_jit_count: count,
            next_slot_ordinal: 0,
        };
        session.population_observation = None;
        session.next_assigned_demand_id = next_id;
        session.active_assigned_demand = Some(ActiveAssignedDemand {
            demand_id,
            remaining_jit_count: count,
        });
        Ok(Some(token))
    }

    /// Generate one generic runner JIT from an exact assigned-population
    /// permit. The permit is tied to this verified session, Scale Set, policy
    /// digest, and one current statistics observation; it is deliberately not
    /// tied to an individual workflow job. Each JIT slot is consumed before
    /// dispatch, and any transport/service error makes this session unresolved
    /// so the caller cannot retry or ACK through the same capability.
    ///
    /// # Errors
    ///
    /// Returns a registration error for a stale or mismatched permit, or the
    /// secret-safe underlying JIT error. A dispatched error consumes the
    /// permit and leaves the session unresolved.
    pub fn jit_assigned_demand<T>(
        &self,
        transport: &mut T,
        session: &mut VerifiedQueueSession,
        demand: &mut VerifiedAssignedDemand,
        runner_name: &str,
    ) -> Result<EncodedJit, SessionError>
    where
        T: DiscoveryTransport + ?Sized,
    {
        self.require_fresh()?;
        self.require_session(session)?;
        let Some(active) = session.active_assigned_demand else {
            return Err(SessionError::Wire(WireError::RegistrationRejected));
        };
        if demand.demand_id != active.demand_id
            || demand.session_id != session.inner.session_id
            || demand.scale_set_id != self.binding.scale_set_id
            || demand.policy_digest != self.policy_digest
            || demand.remaining_jit_count != active.remaining_jit_count
            || demand.remaining_jit_count == 0
        {
            return Err(SessionError::Wire(WireError::RegistrationRejected));
        }
        let next_slot_ordinal = demand
            .next_slot_ordinal
            .checked_add(1)
            .ok_or(SessionError::Wire(WireError::RegistrationRejected))?;

        let request = jit_request(runner_name).map_err(SessionError::from)?;
        bind_admin_origin(transport, self)?;
        demand.remaining_jit_count -= 1;
        demand.next_slot_ordinal = next_slot_ordinal;
        let remaining = active.remaining_jit_count - 1;
        if remaining == 0 {
            session.active_assigned_demand = None;
        } else {
            session.active_assigned_demand = Some(ActiveAssignedDemand {
                demand_id: active.demand_id,
                remaining_jit_count: remaining,
            });
        }

        match jit(
            transport,
            self.binding.scale_set_id,
            self.connection.expose_token(),
            &request,
        ) {
            Ok(encoded) => Ok(encoded),
            Err(error) => {
                demand.remaining_jit_count = 0;
                session.active_assigned_demand = None;
                session.one_shot.assigned_jit_uncertain = true;
                Err(error)
            }
        }
    }
}
