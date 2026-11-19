// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::model::{GenerationState, HeaderAllocation, HeaderLedgerError};
use super::HeaderLedger;

impl HeaderLedger {
    pub(crate) fn settle_zero_header_of_deleted_set(
        &mut self,
        allocation: &HeaderAllocation,
    ) -> Result<(), HeaderLedgerError> {
        let record = self.record_for_allocation_mut(allocation)?;
        if record.in_flight_delete.is_some()
            || (record.delete_sequence != 0 && !record.delete_uncertain)
        {
            return Ok(());
        }
        record.delete_sequence = 0;
        record.delete_uncertain = false;
        record.in_flight_poll = None;
        record.state = GenerationState::Observed;
        record.attempt_sequence = record.attempt_sequence.max(1);
        record.zero_header_settled = true;
        Ok(())
    }
}
