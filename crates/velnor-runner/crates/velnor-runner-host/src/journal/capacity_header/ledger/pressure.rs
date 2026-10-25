use std::time::Instant;

use crate::launch::{FreshPressureSample, adjust_with_sample};

use super::super::model::{
    GenerationState, HeaderAllocation, HeaderLedgerError, HeaderPollAttempt, HeaderRecord,
    PollPurpose,
};
use super::HeaderLedger;

impl HeaderLedger {
    #[cfg(test)]
    pub(crate) fn begin_poll(
        &mut self,
        allocation: &HeaderAllocation,
    ) -> Result<HeaderPollAttempt, HeaderLedgerError> {
        let record = self.record_for_allocation_mut(allocation)?;
        ensure_poll_available(record)?;
        let attempt = Self::begin_attempt(
            record,
            allocation,
            allocation.target,
            PollPurpose::Advertise,
        )?;
        record.pressure_capacity = Some(allocation.target);
        Ok(attempt)
    }

    pub(crate) fn begin_pressure_poll(
        &mut self,
        allocation: &HeaderAllocation,
        occupied_floor: u32,
        sample: Option<FreshPressureSample>,
        now: Instant,
    ) -> Result<HeaderPollAttempt, HeaderLedgerError> {
        let record = self.record_for_allocation_mut(allocation)?;
        ensure_poll_available(record)?;

        let capacity = next_capacity(record, allocation, occupied_floor, sample, now);
        let attempt = Self::begin_attempt(record, allocation, capacity, PollPurpose::Advertise)?;
        record.pressure_capacity = Some(capacity);
        Ok(attempt)
    }
}

fn next_capacity(
    record: &HeaderRecord,
    allocation: &HeaderAllocation,
    occupied_floor: u32,
    sample: Option<FreshPressureSample>,
    now: Instant,
) -> u32 {
    let ceiling = allocation.target;
    if ceiling == 0 {
        return 0;
    }
    let Some(current) = record.pressure_capacity else {
        return ceiling;
    };
    if record.attempt_sequence == 0 {
        return current;
    }
    let fresh = sample.and_then(|sample| sample.sample_if_recent_at(now));
    adjust_with_sample(current, occupied_floor, ceiling, fresh)
}

fn ensure_poll_available(record: &HeaderRecord) -> Result<(), HeaderLedgerError> {
    if record.state == GenerationState::Uncertain {
        return Err(HeaderLedgerError::UnsettledPoll);
    }
    HeaderLedger::ensure_delete_not_started(record)?;
    if record.zero_header_settled {
        return Err(HeaderLedgerError::ZeroHeaderSettled);
    }
    Ok(())
}
