//! Native queue polling, lifecycle intake, and scoped header polling.

mod admission;
mod completion_intake;
mod positive_poll;
mod scoped;

use super::{ack_ready, slot, steps};
#[cfg(test)]
pub(crate) use admission::admission;
#[cfg(test)]
use scoped::finish_scoped_poll_response;
use scoped::poll_scoped_queue;

#[cfg(test)]
pub(crate) use completion_intake::IntakeDecision;
#[cfg(test)]
pub(crate) use positive_poll::Acknowledgement;
#[cfg(test)]
pub(crate) use positive_poll::finish_positive_poll_with_ack;
pub(crate) use positive_poll::{
    CursorDisposition, PositiveAdmission, PositivePollOutcome, Settlement, admit_when_ready,
    positive_poll_once,
};
pub(crate) use scoped::{
    ScopedPollOutcome, ZeroHeaderMode, poll_scoped_connection_once,
    poll_scoped_connection_once_with_mode,
};

#[cfg(test)]
pub(crate) async fn test_started_intake_and_ack<F, Fut>(
    journal: &crate::journal::ConnectionJournal,
    set_id: i64,
    polled: &velnor_runner_github::Poll,
    acknowledge: F,
) -> Result<Vec<i64>, crate::scale_set::EnsureError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), crate::scale_set::EnsureError>>,
{
    completion_intake::intake_and_ack_if_only_scoped(journal, set_id, polled, acknowledge)
        .await
        .map(|decision| decision.started_request_ids)
}

#[cfg(test)]
#[path = "turn/completion_intake_tests.rs"]
mod completion_intake_tests;

#[cfg(test)]
#[path = "turn/completion_quarantine_tests.rs"]
mod completion_quarantine_tests;

#[cfg(test)]
#[path = "turn/scoped_poll_tests.rs"]
pub(super) mod scoped_poll_tests;
