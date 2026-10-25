//! Test-only process runner with bounded output and process-group cleanup.

use std::fmt;
use std::io;
use std::process::Command;
use std::time::Duration;

const CLEANUP_RESERVE: Duration = Duration::from_millis(200);
const POLL_INTERVAL: Duration = Duration::from_millis(4);
const READ_CHUNK: usize = 1024;

/// Result captured from one bounded subprocess.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct BoundedOutput {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) success: bool,
    pub(crate) code: Option<i32>,
    pub(crate) signal: Option<i32>,
}

/// Failure to run, contain, or collect a bounded subprocess.
#[derive(Debug)]
pub(crate) enum BoundedProcessError {
    TimedOut,
    OutputLimit,
    Io(io::Error),
    Cleanup(io::Error),
    #[cfg(not(all(
        unix,
        not(any(
            target_os = "cygwin",
            target_os = "espidf",
            target_os = "fuchsia",
            target_os = "horizon",
            target_os = "openbsd",
            target_os = "redox",
            target_os = "wasi"
        ))
    )))]
    Unsupported,
}

impl fmt::Display for BoundedProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimedOut => formatter.write_str("bounded process timed out"),
            Self::OutputLimit => formatter.write_str("bounded process exceeded output limit"),
            Self::Io(error) => write!(formatter, "bounded process I/O failed: {error}"),
            Self::Cleanup(error) => write!(formatter, "bounded process cleanup failed: {error}"),
            #[cfg(not(all(
                unix,
                not(any(
                    target_os = "cygwin",
                    target_os = "espidf",
                    target_os = "fuchsia",
                    target_os = "horizon",
                    target_os = "openbsd",
                    target_os = "redox",
                    target_os = "wasi"
                ))
            )))]
            Self::Unsupported => formatter.write_str("bounded process is unsupported here"),
        }
    }
}

impl std::error::Error for BoundedProcessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) | Self::Cleanup(error) => Some(error),
            Self::TimedOut | Self::OutputLimit => None,
            #[cfg(not(all(
                unix,
                not(any(
                    target_os = "cygwin",
                    target_os = "espidf",
                    target_os = "fuchsia",
                    target_os = "horizon",
                    target_os = "openbsd",
                    target_os = "redox",
                    target_os = "wasi"
                ))
            )))]
            Self::Unsupported => None,
        }
    }
}

/// Run `command` with a per-stream byte cap and a total absolute deadline.
///
/// On supported Unix targets the child starts in a fresh process group. Both
/// pipes are drained without blocking, and the whole group is killed before
/// the leader is reaped so descendants cannot keep the caller waiting.
#[cfg(all(
    unix,
    not(any(
        target_os = "cygwin",
        target_os = "espidf",
        target_os = "fuchsia",
        target_os = "horizon",
        target_os = "openbsd",
        target_os = "redox",
        target_os = "wasi"
    ))
))]
pub(crate) fn run_bounded(
    command: &mut Command,
    cap_per_stream: usize,
    timeout: Duration,
) -> Result<BoundedOutput, BoundedProcessError> {
    unix_impl::run_bounded(command, cap_per_stream, timeout)
}

/// Unsupported targets fail closed without starting the requested command.
#[cfg(not(all(
    unix,
    not(any(
        target_os = "cygwin",
        target_os = "espidf",
        target_os = "fuchsia",
        target_os = "horizon",
        target_os = "openbsd",
        target_os = "redox",
        target_os = "wasi"
    ))
)))]
pub(crate) fn run_bounded(
    _command: &mut Command,
    _cap_per_stream: usize,
    _timeout: Duration,
) -> Result<BoundedOutput, BoundedProcessError> {
    Err(BoundedProcessError::Unsupported)
}

#[cfg(all(
    unix,
    not(any(
        target_os = "cygwin",
        target_os = "espidf",
        target_os = "fuchsia",
        target_os = "horizon",
        target_os = "openbsd",
        target_os = "redox",
        target_os = "wasi"
    ))
))]
#[path = "impl_cli_bounded_process/unix_impl/mod.rs"]
mod unix_impl;
