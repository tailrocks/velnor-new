//! Portable start telemetry for the fixed obligation report wrapper.

use std::io::{self, Write};

use crate::OrchestratorError;

/// Private operation emitting a task start in Unix milliseconds.
pub const START_OP: &str = "write-task-start-v1";

/// Write a decimal task start to stdout for the wrapper's file handoff.
///
/// An unusable clock emits nothing, preserving unknown telemetry. The
/// generator controls the operation and stdout destination; no command,
/// task graph, or output path is interpreted here.
///
/// # Errors
/// Returns an IO error when stdout cannot accept the stamp.
pub fn write_task_start() -> Result<(), OrchestratorError> {
    write_stamp(&mut io::stdout().lock(), now_ms())
}

/// Shared wall clock for both start capture and report elapsed time.
pub(crate) fn now_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
}

/// Write measured zero literally; unavailable measurements emit no bytes.
fn write_stamp(writer: &mut impl Write, stamp: Option<u64>) -> Result<(), OrchestratorError> {
    if let Some(stamp) = stamp {
        writeln!(writer, "{stamp}")
            .map_err(|error| OrchestratorError::io("task_start_stdout", error.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_preserve_zero_and_unknown() {
        let mut bytes = Vec::new();
        write_stamp(&mut bytes, None).expect("unknown clock");
        assert!(bytes.is_empty());
        write_stamp(&mut bytes, Some(0)).expect("zero clock");
        assert_eq!(bytes, b"0\n");
    }

    #[test]
    fn stdout_failure_is_reported() {
        let error = write_stamp(&mut io::Cursor::new([0u8; 0]), Some(1));
        assert!(error.is_err(), "a rejected stamp must fail");
    }
}
