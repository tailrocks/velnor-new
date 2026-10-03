//! Portable timestamp producer shared by all generated report wrappers.
use super::now_ms;
use crate::OrchestratorError;
use crate::internal::internal;

/// Portable timestamp producer used by generated shell wrappers.
pub const START_TIME_OP: &str = "start-time-v1";

/// Emit a portable Unix millisecond stamp for existing report telemetry.
/// # Errors
/// Fails when the host clock cannot represent Unix milliseconds.
pub fn write_start_time() -> Result<(), OrchestratorError> {
    let stamp = now_ms().ok_or_else(|| internal("clock_unavailable"))?;
    println!("{stamp}");
    Ok(())
}
