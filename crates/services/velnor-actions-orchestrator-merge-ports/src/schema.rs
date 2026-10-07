//! Internal entrypoint schema version, shared by plan and merge.

use velnor_actions_orchestrator_core::{OrchestratorError, internal};

/// Schema version accepted by both internal entrypoints.
pub const SCHEMA: u32 = 1;

/// Reject any schema other than 1.
pub fn check_schema(schema: u32) -> Result<(), OrchestratorError> {
    if schema == SCHEMA {
        Ok(())
    } else {
        Err(internal(&format!("unsupported_schema:{schema}")))
    }
}
