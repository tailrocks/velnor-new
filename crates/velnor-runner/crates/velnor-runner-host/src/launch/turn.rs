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

pub(crate) use positive_poll::{
    CursorDisposition, PositiveAdmission, PositivePollOutcome, Settlement, admit_when_ready,
    positive_poll_once,
};
pub(crate) use scoped::{
    ScopedPollOutcome, ZeroHeaderMode, poll_scoped_connection_once,
    poll_scoped_connection_once_with_mode,
};

#[cfg(test)]
#[path = "turn/completion_intake_tests.rs"]
mod completion_intake_tests;

#[cfg(test)]
#[path = "turn/completion_quarantine_tests.rs"]
mod completion_quarantine_tests;

#[cfg(test)]
#[path = "turn/scoped_poll_tests.rs"]
pub(super) mod scoped_poll_tests;
