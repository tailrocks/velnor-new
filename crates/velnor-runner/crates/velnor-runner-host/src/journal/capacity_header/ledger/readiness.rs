//! Check whether a session remains eligible for a normal queue poll.

use crate::{
    config::ConnectionId,
    journal::capacity_header::model::{GenerationState, PollPurpose},
};

use super::HeaderLedger;

impl HeaderLedger {
    /// Check one exact charged session without changing its allocation.
    pub(crate) fn readiness_poll_eligible(
        &self,
        connection_id: &ConnectionId,
        scale_set_id: i64,
        session_id: &str,
    ) -> bool {
        self.records.values().any(|record| {
            record.identity.connection_id == *connection_id
                && record.identity.scale_set_id == scale_set_id
                && record.identity.session_id.0 == session_id
                && record.state == GenerationState::Observed
                && !record.zero_header_settled
                && !record.delete_uncertain
                && record.in_flight_delete.is_none()
                && record.delete_sequence == 0
                && record.in_flight_poll.is_none_or(|poll| match poll.purpose {
                    PollPurpose::Advertise => poll.header_value == record.target,
                    PollPurpose::ZeroHeader => poll.header_value == 0,
                })
        })
    }
}
