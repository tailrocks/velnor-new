//! Header-allocation state transitions and snapshot validation.

use std::collections::{BTreeMap, BTreeSet};

#[path = "ledger/pressure.rs"]
mod pressure;
#[path = "ledger/readiness.rs"]
mod readiness;
#[path = "ledger/snapshot.rs"]
mod snapshot;
#[path = "ledger/validation.rs"]
mod validation;

use crate::config::ConnectionId;

use super::model::{
    GenerationState, HeaderAllocation, HeaderDeleteAttempt, HeaderGeneration, HeaderLedgerError,
    HeaderPollAttempt, HeaderRecord, HeaderSessionIdentity, InFlightDelete, InFlightPoll,
    MAX_HEADER_CAPACITY, PollOutcome, PollPurpose, SessionId,
};

/// Aggregate reservation ledger for all Scale Set message sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeaderLedger {
    max_jobs: u32,
    next_generation: u64,
    records: BTreeMap<HeaderSessionIdentity, HeaderRecord>,
}

impl HeaderLedger {
    /// Make an empty ledger with one host-wide maximum.
    pub(crate) fn new(max_jobs: u32) -> Result<Self, HeaderLedgerError> {
        if max_jobs == 0 {
            return Err(HeaderLedgerError::ZeroHostCapacity);
        }
        Ok(Self {
            max_jobs,
            next_generation: 1,
            records: BTreeMap::new(),
        })
    }

    /// Reserve aggregate host capacity before sending any poll request.
    ///
    /// A repeated exact identity and target reuses its existing generation.
    /// A changed target or session cannot replace a charged generation.
    pub(crate) fn reserve(
        &mut self,
        connection_id: ConnectionId,
        scale_set_id: i64,
        session_id: &str,
        target: u32,
    ) -> Result<HeaderAllocation, HeaderLedgerError> {
        if scale_set_id <= 0 {
            return Err(HeaderLedgerError::InvalidScaleSetId);
        }
        if target > MAX_HEADER_CAPACITY {
            return Err(HeaderLedgerError::HeaderOutOfRange);
        }
        let identity = HeaderSessionIdentity {
            connection_id,
            scale_set_id,
            session_id: SessionId::parse(session_id)?,
        };
        if let Some(record) = self.records.get(&identity) {
            if record.target != target {
                return Err(HeaderLedgerError::TargetChanged);
            }
            if record.state == GenerationState::Released {
                return Err(HeaderLedgerError::SessionRetired);
            }
            return Ok(record.allocation());
        }
        if self.records.values().any(|record| {
            record.is_charged()
                && record.identity.connection_id == identity.connection_id
                && record.identity.scale_set_id == identity.scale_set_id
        }) {
            return Err(HeaderLedgerError::SessionStillCharged);
        }
        let charged = self
            .checked_charged_capacity()?
            .checked_add(target)
            .ok_or(HeaderLedgerError::CapacityOverflow)?;
        if charged > self.max_jobs {
            return Err(HeaderLedgerError::OverBudget);
        }
        let generation = HeaderGeneration(self.next_generation);
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .ok_or(HeaderLedgerError::GenerationOverflow)?;
        let record = HeaderRecord {
            identity: identity.clone(),
            generation,
            target,
            pressure_capacity: Some(if target == 0 {
                0
            } else {
                crate::launch::advertise(target)
            }),
            state: GenerationState::Pending,
            attempt_sequence: 0,
            in_flight_poll: None,
            zero_header_settled: false,
            delete_sequence: 0,
            in_flight_delete: None,
            delete_uncertain: false,
        };
        let allocation = record.allocation();
        self.records.insert(identity, record);
        Ok(allocation)
    }

    /// Start the required same-session zero-header poll before retirement.
    ///
    /// The allocation remains charged until poll settlement and exact session
    /// deletion are both durably recorded. The response does not echo the
    /// header value; this records completion of the request that sent zero.
    pub(crate) fn begin_zero_poll(
        &mut self,
        allocation: &HeaderAllocation,
    ) -> Result<HeaderPollAttempt, HeaderLedgerError> {
        let record = self.record_for_allocation_mut(allocation)?;
        if record.state == GenerationState::Uncertain {
            return Err(HeaderLedgerError::UnsettledPoll);
        }
        Self::ensure_delete_not_started(record)?;
        let attempt = Self::begin_attempt(record, allocation, 0, PollPurpose::ZeroHeader)?;
        record.zero_header_settled = false;
        Ok(attempt)
    }

    /// Record that one owned poll request returned. No outcome releases charge.
    pub(crate) fn finish_poll(
        &mut self,
        attempt: &HeaderPollAttempt,
        outcome: PollOutcome,
    ) -> Result<(), HeaderLedgerError> {
        let record = self
            .records
            .get_mut(&attempt.identity)
            .ok_or(HeaderLedgerError::UnknownAllocation)?;
        let expected = InFlightPoll {
            sequence: attempt.attempt,
            header_value: attempt.header_value,
            purpose: attempt.purpose,
        };
        if record.generation != attempt.generation || record.in_flight_poll != Some(expected) {
            return Err(HeaderLedgerError::StaleAttempt);
        }
        record.in_flight_poll = None;
        match outcome {
            PollOutcome::Response if record.state != GenerationState::Uncertain => {
                match attempt.purpose {
                    PollPurpose::Advertise => record.state = GenerationState::Observed,
                    PollPurpose::ZeroHeader => {
                        record.state = GenerationState::Observed;
                        record.zero_header_settled = true;
                    }
                }
            }
            PollOutcome::Response => {}
            PollOutcome::Unknown => record.state = GenerationState::Uncertain,
        }
        Ok(())
    }

    /// Persist an exact-session DELETE intent after zero-header settlement.
    pub(crate) fn begin_delete(
        &mut self,
        allocation: &HeaderAllocation,
    ) -> Result<HeaderDeleteAttempt, HeaderLedgerError> {
        let record = self.record_for_allocation_mut(allocation)?;
        if record.state == GenerationState::Uncertain {
            return Err(HeaderLedgerError::UnsettledPoll);
        }
        if record.in_flight_poll.is_some() {
            return Err(HeaderLedgerError::PollStillInFlight);
        }
        if !record.zero_header_settled {
            return Err(HeaderLedgerError::ZeroHeaderNotSettled);
        }
        if record.delete_uncertain {
            return Err(HeaderLedgerError::DeleteOutcomeUncertain);
        }
        if record.in_flight_delete.is_some() {
            return Err(HeaderLedgerError::DeleteAlreadyInFlight);
        }
        if record.delete_sequence != 0 {
            return Err(HeaderLedgerError::DeleteNotReady);
        }
        let sequence = record
            .delete_sequence
            .checked_add(1)
            .ok_or(HeaderLedgerError::AttemptOverflow)?;
        record.delete_sequence = sequence;
        record.in_flight_delete = Some(InFlightDelete { sequence });
        Ok(HeaderDeleteAttempt {
            identity: allocation.identity.clone(),
            generation: allocation.generation,
            sequence,
        })
    }

    /// Keep a timed-out or otherwise ambiguous deletion charged permanently.
    pub(crate) fn mark_delete_unknown(
        &mut self,
        attempt: &HeaderDeleteAttempt,
    ) -> Result<(), HeaderLedgerError> {
        let record = self.record_for_delete_attempt_mut(attempt)?;
        record.in_flight_delete = None;
        record.delete_uncertain = true;
        Ok(())
    }

    /// Release one generation after its source adapter proves exact DELETE 204.
    pub(crate) fn finish_delete(
        &mut self,
        attempt: &HeaderDeleteAttempt,
    ) -> Result<(), HeaderLedgerError> {
        let record = self.record_for_delete_attempt_mut(attempt)?;
        if record.state == GenerationState::Uncertain {
            return Err(HeaderLedgerError::UnsettledPoll);
        }
        if record.in_flight_poll.is_some() {
            return Err(HeaderLedgerError::PollStillInFlight);
        }
        if !record.zero_header_settled {
            return Err(HeaderLedgerError::ZeroHeaderNotSettled);
        }
        record.in_flight_delete = None;
        record.state = GenerationState::Released;
        Ok(())
    }

    fn record_for_delete_attempt_mut(
        &mut self,
        attempt: &HeaderDeleteAttempt,
    ) -> Result<&mut HeaderRecord, HeaderLedgerError> {
        let record = self
            .records
            .get_mut(&attempt.identity)
            .ok_or(HeaderLedgerError::UnknownAllocation)?;
        if record.generation != attempt.generation
            || record.in_flight_delete
                != Some(InFlightDelete {
                    sequence: attempt.sequence,
                })
        {
            return Err(HeaderLedgerError::StaleAttempt);
        }
        Ok(record)
    }

    /// Return the sum of all unreleased header targets.
    #[must_use]
    pub(crate) fn charged_capacity(&self) -> u32 {
        self.checked_charged_capacity().unwrap_or(u32::MAX)
    }

    /// Return the host-wide capacity ceiling.
    #[must_use]
    pub(crate) const fn max_jobs(&self) -> u32 {
        self.max_jobs
    }

    /// Return every charged allocation in deterministic identity order.
    #[must_use]
    pub(crate) fn charged_allocations(&self) -> Vec<HeaderAllocation> {
        self.records
            .values()
            .filter(|record| record.is_charged())
            .map(HeaderRecord::allocation)
            .collect()
    }

    /// Return the connection with the newest positive allocation generation.
    #[must_use]
    pub(crate) fn fairness_cursor(&self) -> Option<ConnectionId> {
        self.records
            .values()
            .filter(|record| record.target > 0)
            .max_by_key(|record| record.generation)
            .map(|record| record.identity.connection_id.clone())
    }

    /// Change the host ceiling only when every existing header still fits.
    pub(crate) fn set_max_jobs(&mut self, max_jobs: u32) -> Result<(), HeaderLedgerError> {
        if max_jobs == 0 {
            return Err(HeaderLedgerError::ZeroHostCapacity);
        }
        if self.checked_charged_capacity()? > max_jobs {
            return Err(HeaderLedgerError::OverBudget);
        }
        self.max_jobs = max_jobs;
        Ok(())
    }

    fn record_for_allocation_mut(
        &mut self,
        allocation: &HeaderAllocation,
    ) -> Result<&mut HeaderRecord, HeaderLedgerError> {
        let record = self
            .records
            .get_mut(&allocation.identity)
            .ok_or(HeaderLedgerError::UnknownAllocation)?;
        if record.generation != allocation.generation || record.target != allocation.target {
            return Err(HeaderLedgerError::StaleAllocation);
        }
        if record.state == GenerationState::Released {
            return Err(HeaderLedgerError::SessionRetired);
        }
        Ok(record)
    }

    fn begin_attempt(
        record: &mut HeaderRecord,
        allocation: &HeaderAllocation,
        header_value: u32,
        purpose: PollPurpose,
    ) -> Result<HeaderPollAttempt, HeaderLedgerError> {
        if record.in_flight_poll.is_some() {
            return Err(HeaderLedgerError::PollAlreadyInFlight);
        }
        let attempt = record
            .attempt_sequence
            .checked_add(1)
            .ok_or(HeaderLedgerError::AttemptOverflow)?;
        record.attempt_sequence = attempt;
        record.in_flight_poll = Some(InFlightPoll {
            sequence: attempt,
            header_value,
            purpose,
        });
        Ok(HeaderPollAttempt {
            identity: allocation.identity.clone(),
            generation: allocation.generation,
            attempt,
            header_value,
            purpose,
        })
    }

    fn ensure_delete_not_started(record: &HeaderRecord) -> Result<(), HeaderLedgerError> {
        if record.delete_uncertain {
            return Err(HeaderLedgerError::DeleteOutcomeUncertain);
        }
        if record.in_flight_delete.is_some() {
            return Err(HeaderLedgerError::DeleteAlreadyInFlight);
        }
        if record.delete_sequence != 0 {
            return Err(HeaderLedgerError::DeleteNotReady);
        }
        Ok(())
    }

    fn has_multiple_live_sessions_per_scale_set(&self) -> bool {
        let mut seen = BTreeSet::new();
        self.records
            .values()
            .filter(|record| record.is_charged())
            .any(|record| {
                !seen.insert((
                    record.identity.connection_id.clone(),
                    record.identity.scale_set_id,
                ))
            })
    }

    fn checked_charged_capacity(&self) -> Result<u32, HeaderLedgerError> {
        self.records
            .values()
            .filter(|record| record.is_charged())
            .try_fold(0_u32, |total, record| {
                total
                    .checked_add(record.target)
                    .ok_or(HeaderLedgerError::CapacityOverflow)
            })
    }
}
