//! Pure state machine for durable `X-ScaleSetMaxCapacity` allocations.
//!
//! Every reserved header remains charged until its owned polls are durably
//! settled, a same-session zero-header request completes, and an exact
//! source-backed session DELETE is acknowledged.

#[path = "capacity_header/ledger.rs"]
mod ledger;
#[path = "capacity_header/model.rs"]
mod model;

pub(crate) use ledger::HeaderLedger;
#[cfg(test)]
use model::{GenerationState, SessionId};
pub(crate) use model::{
    HeaderAllocation, HeaderGeneration, HeaderLedgerError, HeaderLedgerSnapshot, HeaderPollAttempt,
    HeaderSessionIdentity, PollOutcome, PollsSettledProof, SessionDeleteProof,
};

#[cfg(test)]
#[path = "capacity_header/tests.rs"]
mod tests;
