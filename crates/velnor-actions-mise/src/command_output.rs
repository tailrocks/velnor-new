//! Typed child-process result: captured streams, exit decoding, and the
//! bounded pipe readers backing `IsolatedCommand::run_bounded`.
//!
//! Declared from `command.rs` (`#[path]`, no `lib.rs` edit); `command.rs`
//! re-exports the public surface so `command::X` paths keep working.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::MiseError;

/// External cancellation handle for one subprocess run (P07-5).
///
/// Beyond the internal deadline: any thread holding a shared handle may
/// abort the run, and the child is killed at the next poll. Cancellation
/// surfaces as typed [`MiseError::SpawnFailed`], never as a task outcome.
#[derive(Debug, Default)]
pub struct CancelHandle {
    /// Cancellation flag shared with the polling run loop.
    cancelled: AtomicBool,
}

impl CancelHandle {
    /// New uncancelled handle.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation; the run loop kills the child at its next poll.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// `SpawnFailed` message for external cancellation (P07-5).
pub const SPAWN_CANCELLED_MESSAGE: &str = "cancelled";

/// `SpawnFailed` message prefix for deadline expiry; seconds are appended.
pub const SPAWN_TIMEOUT_MESSAGE_PREFIX: &str = "timeout_after_secs:";

/// Whether a command error is timeout/cancellation, not a task outcome.
///
/// Both abortions surface as typed [`MiseError::SpawnFailed`]: they must
/// propagate to the caller and never degrade into a normal cache miss.
#[must_use]
pub fn is_cancel_or_timeout(error: &MiseError) -> bool {
    matches!(error, MiseError::SpawnFailed { message, .. }
        if message == SPAWN_CANCELLED_MESSAGE
            || message.starts_with(SPAWN_TIMEOUT_MESSAGE_PREFIX))
}

/// Typed child-process result: captured streams plus a typed exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    /// Captured standard output bytes (bounded by the spawn cap).
    pub stdout: Vec<u8>,
    /// Captured standard error bytes (bounded by the spawn cap).
    pub stderr: Vec<u8>,
    /// Exit code when the child exited normally.
    pub code: Option<i32>,
    /// Terminating signal number when killed by a signal (Unix only).
    pub signal: Option<i32>,
    /// Whether the exit status reports success.
    pub success: bool,
}

impl ProcessOutput {
    /// Fail with [`MiseError::NonZeroExit`] unless the status reports success.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::NonZeroExit`] when the status reports failure.
    pub fn require_success(&self, program: &str) -> Result<&Self, MiseError> {
        if self.success {
            return Ok(self);
        }
        Err(MiseError::NonZeroExit {
            program: program.to_owned(),
            code: self.code,
            stderr: String::from_utf8_lossy(&self.stderr).into_owned(),
        })
    }

    /// Decode standard output as UTF-8 text.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidUtf8`] when stdout is not valid UTF-8.
    pub fn stdout_text(&self, program: &str) -> Result<String, MiseError> {
        String::from_utf8(self.stdout.clone()).map_err(|_| MiseError::InvalidUtf8 {
            program: program.to_owned(),
            stream: "stdout".to_owned(),
        })
    }
}

/// Redact `--token <value>` argv pairs for `Debug`: the flag stays so
/// the shape is visible, the following value becomes `<redacted>`.
/// Every other argument renders unchanged.
#[must_use]
pub(crate) fn redact_argv_for_debug(argv: &[std::ffi::OsString]) -> Vec<String> {
    let mut out = Vec::with_capacity(argv.len());
    let mut hide_next = false;
    for arg in argv {
        let text = arg.to_string_lossy().into_owned();
        if hide_next {
            out.push("<redacted>".to_owned());
            hide_next = false;
        } else {
            hide_next = text == "--token";
            out.push(text);
        }
    }
    out
}

pub(crate) fn read_capped<R: std::io::Read>(pipe: Option<R>, limit: usize) -> (Vec<u8>, bool) {
    let Some(pipe) = pipe else {
        return (Vec::new(), false);
    };
    let mut buf = Vec::new();
    let capped = pipe
        .take(limit.saturating_add(1).try_into().unwrap_or(u64::MAX))
        .read_to_end(&mut buf)
        .is_err()
        || buf.len() > limit;
    (buf, capped)
}

pub(crate) fn signal_of(status: std::process::ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        std::os::unix::process::ExitStatusExt::signal(&status)
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        None
    }
}
