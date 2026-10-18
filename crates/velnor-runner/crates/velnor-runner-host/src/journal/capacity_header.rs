//! Pure state machine for durable `X-ScaleSetMaxCapacity` allocations.
//!
//! Every reserved header remains charged until its owned polls are durably
//! settled, a same-session zero-header request completes, and an exact
//! source-backed session DELETE is acknowledged.

#[path = "capacity_header/ledger.rs"]
mod ledger;
#[path = "capacity_header/model.rs"]
mod model;
#[path = "capacity_header/storage.rs"]
mod storage;

pub(crate) use ledger::HeaderLedger;
#[cfg(test)]
use model::{GenerationState, HeaderGeneration, SessionId};
pub(crate) use model::{
    HeaderAllocation, HeaderDeleteAttempt, HeaderLedgerError, HeaderLedgerSnapshot,
    HeaderPollAttempt, PollOutcome,
};
pub(super) use storage::{
    bootstrap, bootstrap_schema_connection, has_any_charged, has_charged_connection, load, save,
    transition, validate, validate_capacity_policy_connection, validate_connection,
};

#[cfg(test)]
#[path = "capacity_header/tests.rs"]
mod tests;
