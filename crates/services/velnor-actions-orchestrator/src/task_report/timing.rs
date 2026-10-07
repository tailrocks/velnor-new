//! Parsing and deriving wrapper-captured task timing telemetry.

use super::OrchestratorError;
use velnor_actions_orchestrator_core::internal;

/// Parse a captured `$?` value: an integer in the 8-bit exit range.
pub(super) fn parse_exit_code(raw: &str) -> Result<i32, OrchestratorError> {
    raw.parse::<i32>()
        .ok()
        .filter(|code| (0..=255).contains(code))
        .ok_or_else(|| internal("bad_exit_code"))
}

/// Parse a wrapper-captured unix-millis start; `None` when absent.
///
/// Garbage fails closed to unmeasured (`None`): a malformed stamp must
/// never error the report nor fabricate a duration.
pub(super) fn parse_start_ms(raw: &str) -> Option<u64> {
    raw.parse::<u64>().ok()
}

/// Wall-clock now in unix millis; `None` when the clock is unusable.
pub(super) fn now_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
}

/// Measured elapsed millis, or `None` when telemetry is absent.
///
/// A present, non-future start always measures at least 1ms: absent,
/// unparseable, future, or clockless telemetry stays unmeasured and
/// is never labeled as measured.
pub(super) fn elapsed_ms(start_ms: Option<u64>) -> Option<u64> {
    let (Some(start), Some(now)) = (start_ms, now_ms()) else {
        return None;
    };
    now.checked_sub(start).map(|elapsed| elapsed.max(1))
}
