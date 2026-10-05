//! Single-flight refresh. HTTP 403 is not retried.

use std::sync::Mutex;

use crate::error::WireError;

/// How many 401 refreshes have started.
#[derive(Debug)]
pub struct RefreshGate {
    started: Mutex<u32>,
}

impl RefreshGate {
    /// No refresh has run.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            started: Mutex::new(0),
        }
    }

    /// Count of refreshes started.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::Poisoned`] if the mutex is poisoned.
    pub fn started(&self) -> Result<u32, WireError> {
        self.started
            .lock()
            .map(|guard| *guard)
            .map_err(|_| WireError::Poisoned)
    }
}

impl Default for RefreshGate {
    fn default() -> Self {
        Self::new()
    }
}

/// Classification of a pinned status code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusClass {
    /// HTTP 200.
    Ok,
    /// HTTP 202. Empty poll, not an error, not an ack.
    EmptyPoll,
    /// HTTP 204 ack.
    Acked,
    /// HTTP 409. Do not delete the other session.
    SessionConflict,
    /// HTTP 401. One refresh is allowed.
    RefreshOnce,
}

/// Classify a status. A second 401 fails. 403 fails without consuming a retry loop.
///
/// # Errors
///
/// Returns [`WireError::RefreshExhausted`], [`WireError::Forbidden`],
/// [`WireError::UnexpectedStatus`], or [`WireError::Poisoned`].
pub fn classify_status(status: u16, gate: &RefreshGate) -> Result<StatusClass, WireError> {
    match status {
        200 => Ok(StatusClass::Ok),
        202 => Ok(StatusClass::EmptyPoll),
        204 => Ok(StatusClass::Acked),
        409 => Ok(StatusClass::SessionConflict),
        401 => begin_refresh(gate),
        403 => Err(WireError::Forbidden),
        _ => Err(WireError::UnexpectedStatus),
    }
}

fn begin_refresh(gate: &RefreshGate) -> Result<StatusClass, WireError> {
    let mut started = gate.started.lock().map_err(|_| WireError::Poisoned)?;
    if *started >= 1 {
        return Err(WireError::RefreshExhausted);
    }
    *started += 1;
    Ok(StatusClass::RefreshOnce)
}
