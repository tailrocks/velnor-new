// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::{ConnectionJournal, HeaderAllocation, HeaderDeleteAttempt, HeaderPollAttempt};
use crate::scale_set::EnsureError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionDeleteOutcome {
    NotReady,
    Deleted,
    Held,
}

pub(crate) struct CompletedHeaderPoll {
    attempt: HeaderPollAttempt,
}

impl CompletedHeaderPoll {
    pub(in crate::launch) fn after_response(
        attempt: HeaderPollAttempt,
        allocation: &HeaderAllocation,
        journal: &ConnectionJournal,
        scale_set_id: i64,
        session_id: &str,
    ) -> Result<Self, EnsureError> {
        if !allocation.matches_session(journal.connection_id(), scale_set_id, session_id)
            || !attempt.matches_session(journal.connection_id(), scale_set_id, session_id)
            || (attempt.header_value() != 0 && attempt.header_value() > allocation.target())
        {
            return Err(EnsureError::AdmissionClosed);
        }
        Ok(Self { attempt })
    }

    pub(crate) fn into_attempt(self) -> HeaderPollAttempt {
        self.attempt
    }
}

pub(crate) struct DeletedSession204 {
    attempt: HeaderDeleteAttempt,
}

impl DeletedSession204 {
    pub(super) fn after_response(
        attempt: HeaderDeleteAttempt,
        allocation: &HeaderAllocation,
        journal: &ConnectionJournal,
        scale_set_id: i64,
        session_id: &str,
    ) -> Result<Self, EnsureError> {
        if !allocation.matches_session(journal.connection_id(), scale_set_id, session_id)
            || !attempt.matches_session(journal.connection_id(), scale_set_id, session_id)
        {
            return Err(EnsureError::AdmissionClosed);
        }
        Ok(Self { attempt })
    }

    pub(crate) fn into_attempt(self) -> HeaderDeleteAttempt {
        self.attempt
    }
}
