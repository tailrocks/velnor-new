use crate::journal::{ConnectionJournal, HeaderAllocation, HeaderDeleteAttempt, HeaderPollAttempt};
use crate::scale_set::EnsureError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionDeleteOutcome {
    NotReady,
    Deleted,
    Held,
}

/// Proof that the exact allocated poll returned and its response was handled.
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
            || (attempt.header_value() != 0 && allocation.target() != attempt.header_value())
        {
            return Err(EnsureError::AdmissionClosed);
        }
        Ok(Self { attempt })
    }

    pub(crate) fn into_attempt(self) -> HeaderPollAttempt {
        self.attempt
    }

    #[cfg(test)]
    pub(crate) fn after_test_response(
        attempt: HeaderPollAttempt,
        allocation: &HeaderAllocation,
        journal: &ConnectionJournal,
        scale_set_id: i64,
        session_id: &str,
    ) -> Result<Self, EnsureError> {
        Self::after_response(attempt, allocation, journal, scale_set_id, session_id)
    }

    #[cfg(test)]
    pub(crate) fn test_only(attempt: HeaderPollAttempt) -> Self {
        Self { attempt }
    }
}

/// Proof that GitHub returned the exact successful 204 for an owned session DELETE.
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

    #[cfg(test)]
    pub(crate) fn test_only(attempt: HeaderDeleteAttempt) -> Self {
        Self { attempt }
    }
}
