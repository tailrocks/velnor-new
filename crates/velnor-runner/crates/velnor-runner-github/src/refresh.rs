use std::sync::Mutex;

use crate::error::WireError;

#[derive(Debug)]
pub struct RefreshGate {
    started: Mutex<u32>,
}

impl RefreshGate {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            started: Mutex::new(0),
        }
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusClass {
    Ok,

    EmptyPoll,

    Acked,

    SessionConflict,

    RefreshOnce,
}

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
