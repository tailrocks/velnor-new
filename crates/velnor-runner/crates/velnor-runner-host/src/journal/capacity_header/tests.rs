use std::error::Error;

use super::{
    GenerationState, HeaderAllocation, HeaderGeneration, HeaderLedger, HeaderLedgerError,
    PollOutcome, SessionId,
};
use crate::config::ConnectionId;
use crate::error::HostError;

type TestResult = Result<(), Box<dyn Error>>;

fn connection(value: &str) -> Result<ConnectionId, HostError> {
    ConnectionId::parse(value)
}

fn reserve(
    ledger: &mut HeaderLedger,
    connection_id: ConnectionId,
    scale_set_id: i64,
    session_id: &str,
    target: u32,
) -> Result<HeaderAllocation, HeaderLedgerError> {
    ledger.reserve(connection_id, scale_set_id, session_id, target)
}

#[path = "tests/recovery_cases.rs"]
mod recovery_cases;
#[path = "tests/reservation_cases.rs"]
mod reservation_cases;
