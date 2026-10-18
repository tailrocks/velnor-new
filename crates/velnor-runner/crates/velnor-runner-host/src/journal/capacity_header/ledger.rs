//! Header-allocation state transitions and snapshot validation.

use std::collections::{BTreeMap, BTreeSet};

use crate::config::ConnectionId;

use super::model::{
    GenerationState, HeaderAllocation, HeaderGeneration, HeaderLedgerError, HeaderLedgerSnapshot,
    HeaderPollAttempt, HeaderRecord, HeaderSessionIdentity, InFlightPoll, MAX_HEADER_CAPACITY,
    MAX_SESSION_ID_BYTES, PollOutcome, PollPurpose, PollsSettledProof, SessionDeleteProof,
    SessionId,
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
            state: GenerationState::Pending,
            attempt_sequence: 0,
            in_flight_poll: None,
            zero_header_settled: false,
        };
        let allocation = record.allocation();
        self.records.insert(identity, record);
        Ok(allocation)
    }

    /// Persist an in-flight marker before issuing the corresponding GET.
    pub(crate) fn begin_poll(
        &mut self,
        allocation: &HeaderAllocation,
    ) -> Result<HeaderPollAttempt, HeaderLedgerError> {
        let record = self.record_for_allocation_mut(allocation)?;
        if record.state == GenerationState::Uncertain {
            return Err(HeaderLedgerError::UnsettledPoll);
        }
        if record.zero_header_settled {
            return Err(HeaderLedgerError::ZeroHeaderSettled);
        }
        Self::begin_attempt(
            record,
            allocation,
            allocation.target,
            PollPurpose::Advertise,
        )
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
        if record.zero_header_settled {
            return Err(HeaderLedgerError::ZeroHeaderSettled);
        }
        Self::begin_attempt(record, allocation, 0, PollPurpose::ZeroHeader)
    }

    /// Record that one owned poll request returned. No outcome releases charge.
    pub(crate) fn finish_poll(
        &mut self,
        attempt: HeaderPollAttempt,
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

    /// Release one exact generation after poll settlement and session deletion.
    ///
    /// Production proof construction is intentionally unavailable. A reviewed
    /// adapter must bind poll settlement and the exact session DELETE/204 to
    /// this generation. A DELETE cannot settle an earlier unknown long poll.
    pub(crate) fn release(
        &mut self,
        polls_settled: &PollsSettledProof,
        deletion: &SessionDeleteProof,
    ) -> Result<(), HeaderLedgerError> {
        if polls_settled.identity != deletion.identity
            || polls_settled.generation != deletion.generation
        {
            return Err(HeaderLedgerError::ReleaseEvidenceMismatch);
        }
        let record = self
            .records
            .get_mut(&polls_settled.identity)
            .ok_or(HeaderLedgerError::UnknownAllocation)?;
        if record.generation != polls_settled.generation {
            return Err(HeaderLedgerError::ReleaseEvidenceMismatch);
        }
        if record.in_flight_poll.is_some() {
            return Err(HeaderLedgerError::PollStillInFlight);
        }
        if record.state == GenerationState::Uncertain {
            return Err(HeaderLedgerError::UnsettledPoll);
        }
        if !record.zero_header_settled {
            return Err(HeaderLedgerError::ZeroHeaderNotSettled);
        }
        if record.state == GenerationState::Released {
            return Err(HeaderLedgerError::SessionRetired);
        }
        record.state = GenerationState::Released;
        Ok(())
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

    /// Copy a deterministic snapshot for durable journal storage.
    #[must_use]
    pub(crate) fn snapshot(&self) -> HeaderLedgerSnapshot {
        HeaderLedgerSnapshot {
            max_jobs: self.max_jobs,
            next_generation: self.next_generation,
            records: self.records.values().cloned().collect(),
        }
    }

    /// Restore and validate persisted state; an interrupted poll stays charged.
    pub(crate) fn restore(snapshot: HeaderLedgerSnapshot) -> Result<Self, HeaderLedgerError> {
        Self::restore_with_poll_recovery(snapshot, true)
    }

    /// Restore a validated snapshot inside a running process transaction.
    ///
    /// Unlike [`Self::restore`], this keeps a persisted in-flight poll so its
    /// owning request can durably settle it. Startup recovery must use
    /// [`Self::restore`] to mark an interrupted request uncertain.
    pub(crate) fn restore_live(snapshot: HeaderLedgerSnapshot) -> Result<Self, HeaderLedgerError> {
        Self::restore_with_poll_recovery(snapshot, false)
    }

    fn restore_with_poll_recovery(
        mut snapshot: HeaderLedgerSnapshot,
        recover_interrupted_polls: bool,
    ) -> Result<Self, HeaderLedgerError> {
        if snapshot.max_jobs == 0 || snapshot.next_generation == 0 {
            return Err(HeaderLedgerError::InvalidSnapshot);
        }
        let mut records = BTreeMap::new();
        let mut generations = BTreeSet::new();
        let mut largest_generation = 0;
        for mut record in snapshot.records.drain(..) {
            if !valid_record(&record) || !generations.insert(record.generation) {
                return Err(HeaderLedgerError::InvalidSnapshot);
            }
            largest_generation = largest_generation.max(record.generation.0);
            if recover_interrupted_polls && record.in_flight_poll.take().is_some() {
                record.state = GenerationState::Uncertain;
                record.zero_header_settled = false;
            }
            if records.insert(record.identity.clone(), record).is_some() {
                return Err(HeaderLedgerError::InvalidSnapshot);
            }
        }
        if snapshot.next_generation <= largest_generation {
            return Err(HeaderLedgerError::InvalidSnapshot);
        }
        let ledger = Self {
            max_jobs: snapshot.max_jobs,
            next_generation: snapshot.next_generation,
            records,
        };
        if ledger.checked_charged_capacity()? > ledger.max_jobs
            || ledger.has_multiple_live_sessions_per_scale_set()
        {
            return Err(HeaderLedgerError::InvalidSnapshot);
        }
        Ok(ledger)
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

fn valid_record(record: &HeaderRecord) -> bool {
    let sequence = record.attempt_sequence;
    let in_flight = record.in_flight_poll;
    let sequence_valid = match record.state {
        GenerationState::Pending => match in_flight {
            Some(_) => sequence > 0,
            None => sequence == 0,
        },
        GenerationState::Observed => sequence > 0,
        GenerationState::Uncertain => sequence > 0 && in_flight.is_none(),
        GenerationState::Released => {
            sequence > 0 && in_flight.is_none() && record.zero_header_settled
        }
    };
    let zero_poll_valid = !record.zero_header_settled
        || matches!(
            record.state,
            GenerationState::Observed | GenerationState::Released
        ) && in_flight.is_none();
    let in_flight_valid = in_flight.is_none_or(|poll| {
        poll.sequence == sequence
            && poll.header_value <= MAX_HEADER_CAPACITY
            && match poll.purpose {
                PollPurpose::Advertise => poll.header_value == record.target,
                PollPurpose::ZeroHeader => poll.header_value == 0 && !record.zero_header_settled,
            }
    });
    record.identity.scale_set_id > 0
        && !record.identity.session_id.0.is_empty()
        && record.identity.session_id.0.len() <= MAX_SESSION_ID_BYTES
        && !record.identity.session_id.0.chars().any(char::is_control)
        && record.target <= MAX_HEADER_CAPACITY
        && record.generation.0 > 0
        && sequence_valid
        && zero_poll_valid
        && in_flight_valid
}
