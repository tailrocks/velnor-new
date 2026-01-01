//! Task digest binding argv plus toolchain for one obligation.

use serde::Serialize;
use velnor_actions_contract::ContractError;

use super::snapshot::canonical_digest;

/// Task digest binding argv plus toolchain for one obligation.
///
/// Shared by event-time plan obligations and static crate-job
/// obligations so both judge the same digest.
///
/// # Errors
///
/// Returns [`ContractError`] when canonical serialization fails.
pub fn task_digest(
    task_id: &str,
    argv: &[String],
    toolchain_id: &str,
) -> Result<String, ContractError> {
    canonical_digest(&TaskDigestInputs {
        task_id,
        argv,
        toolchain_id,
    })
}

/// Task-digest preimage fields.
#[derive(Debug, Serialize)]
struct TaskDigestInputs<'a> {
    /// Stable task ID.
    task_id: &'a str,
    /// Fixed argument vector.
    argv: &'a [String],
    /// Toolchain identity digest.
    toolchain_id: &'a str,
}
