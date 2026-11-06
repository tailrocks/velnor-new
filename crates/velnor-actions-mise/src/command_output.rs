// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::MiseError;

#[derive(Debug, Default)]
pub struct CancelHandle {
    cancelled: AtomicBool,
}

impl CancelHandle {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

pub const SPAWN_CANCELLED_MESSAGE: &str = "cancelled";

pub const SPAWN_TIMEOUT_MESSAGE_PREFIX: &str = "timeout_after_secs:";
pub const SPAWN_DEADLINE_MESSAGE: &str = "timeout_after_absolute_deadline";
const CLEANUP_FAILURE_DELIMITER: &str = ";cleanup_failed:";
pub(super) const MAX_CLEANUP_FAILURE_BYTES: usize = 1024;

#[must_use]
pub fn is_cancel_or_timeout(error: &MiseError) -> bool {
    let MiseError::SpawnFailed { message, .. } = error else {
        return false;
    };
    let primary = message
        .split_once(CLEANUP_FAILURE_DELIMITER)
        .filter(|(_, details)| {
            !details.is_empty()
                && details.len() <= MAX_CLEANUP_FAILURE_BYTES
                && !details.contains(CLEANUP_FAILURE_DELIMITER)
        })
        .map_or(message.as_str(), |(primary, _)| primary);
    primary == SPAWN_CANCELLED_MESSAGE
        || primary == SPAWN_DEADLINE_MESSAGE
        || primary.starts_with(SPAWN_TIMEOUT_MESSAGE_PREFIX)
}

#[must_use]
pub fn is_cleanup_failure(error: &MiseError) -> bool {
    matches!(error, MiseError::SpawnFailed { message, .. }
        if message.contains(CLEANUP_FAILURE_DELIMITER))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub success: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundedCommandStatus {
    NotStarted,
    Exited(i32),
    Signaled(i32),
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundedCommandTermination {
    Completed,
    TimedOut,
    OutputLimitExceeded,
    Cancelled,
    SpawnFailed,
    ExecutionFailed,
    CleanupFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedCommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub status: BoundedCommandStatus,
    pub termination: BoundedCommandTermination,
    pub execution_error: Option<String>,
    pub cleanup_error: Option<String>,
}

#[derive(Debug)]
#[must_use]
pub struct ProcessGroupDrainReceipt {
    _private: (),
}

impl ProcessGroupDrainReceipt {
    pub(crate) fn new() -> Self {
        Self { _private: () }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessFileOutput {
    pub stdout_bytes: u64,
    pub stderr: Vec<u8>,
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub success: bool,
}

impl ProcessOutput {
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

    pub fn stdout_text(&self, program: &str) -> Result<String, MiseError> {
        String::from_utf8(self.stdout.clone()).map_err(|_| MiseError::InvalidUtf8 {
            program: program.to_owned(),
            stream: "stdout".to_owned(),
        })
    }
}

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
