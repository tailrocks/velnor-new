//! Typed child-process result: captured streams, exit decoding, and the
//! bounded pipe readers backing `IsolatedCommand::run_bounded`.
//!
//! Declared from `command.rs` (`#[path]`, no `lib.rs` edit); `command.rs`
//! re-exports the public surface so `command::X` paths keep working.

use std::io::Read;

use crate::error::MiseError;

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
