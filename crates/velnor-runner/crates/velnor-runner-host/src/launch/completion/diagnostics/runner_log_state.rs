use serde::{Deserialize, Serialize};

use super::{DIRECTORY_MODE, FILE_MODE, check_capture_state};
use crate::error::HostError;

const PROJECTION_BYTES: usize = 512 * 1024;
const STATE_BYTES: usize = 8 * 1024;
const TEMP_FILE: &str = ".runner-log-capture.tmp";

#[path = "runner_log_state/persistence.rs"]
mod persistence;
#[path = "runner_log_state/storage.rs"]
mod storage;
#[cfg(test)]
#[path = "runner_log_state/tests.rs"]
mod tests;

pub(in crate::launch) use persistence::{
    CaptureStateRequest, binding, read_projection, read_state, recover_terminal_projection,
};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::launch) struct CaptureBinding {
    pub(in crate::launch) journal_identity: String,
    pub(in crate::launch) connection_id: String,
    pub(in crate::launch) intent_id: i64,
    pub(in crate::launch) reservation_id: i64,
    pub(in crate::launch) operation_id: i64,
    pub(in crate::launch) topology_version: i64,
    pub(in crate::launch) engine_id: String,
    pub(in crate::launch) docker_root_digest: String,
    pub(in crate::launch) runner_container_id: String,
    pub(in crate::launch) dind_container_id: String,
    pub(in crate::launch) runner_home_volume: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::launch) enum CapturePhase {
    Prepared,
    Created,
    StartRequested,
    Terminal,
    ProjectionPending,
    Complete,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::launch) struct CaptureState {
    pub(in crate::launch) schema: u32,
    pub(in crate::launch) binding: CaptureBinding,
    pub(in crate::launch) attempt: i64,
    pub(in crate::launch) create_requests: u8,
    pub(in crate::launch) start_requests: u8,
    pub(in crate::launch) phase: CapturePhase,
    pub(in crate::launch) exec_id: Option<String>,
    pub(in crate::launch) exit_code: Option<i64>,
    pub(in crate::launch) projection_sha256: Option<String>,
    pub(in crate::launch) projection_bytes: Option<u64>,
    pub(in crate::launch) has_trace: Option<bool>,
}

impl CaptureState {
    pub(in crate::launch) fn validate(&self, binding: &CaptureBinding) -> Result<(), HostError> {
        let exec_required = !matches!(self.phase, CapturePhase::Prepared);
        let terminal_required = matches!(
            self.phase,
            CapturePhase::Terminal
                | CapturePhase::ProjectionPending
                | CapturePhase::Complete
                | CapturePhase::Unavailable
        );
        let projection_required = matches!(
            self.phase,
            CapturePhase::ProjectionPending | CapturePhase::Complete | CapturePhase::Unavailable
        );
        let invalid_exec_id = match (&self.exec_id, exec_required) {
            (None, false) => false,
            (Some(id), true) => !valid_id(id),
            _ => true,
        };
        let phase_attempts_valid = match self.phase {
            CapturePhase::Prepared => self.create_requests <= 3 && self.start_requests == 0,
            CapturePhase::Created => self.create_requests > 0 && self.start_requests == 0,
            CapturePhase::StartRequested => {
                self.create_requests > 0 && (1..=3).contains(&self.start_requests)
            }
            CapturePhase::Terminal
            | CapturePhase::ProjectionPending
            | CapturePhase::Complete
            | CapturePhase::Unavailable => {
                self.create_requests > 0 && (1..=3).contains(&self.start_requests)
            }
        };
        if self.schema != 3
            || &self.binding != binding
            || self.attempt <= 0
            || self.attempt > 3
            || !phase_attempts_valid
            || invalid_exec_id
            || self.exit_code.is_some() != terminal_required
            || self
                .exit_code
                .is_some_and(|code| !(0..=255).contains(&code))
            || self.projection_sha256.is_some() != projection_required
            || self.projection_bytes.is_some() != projection_required
            || self.has_trace.is_some() != projection_required
            || self
                .projection_sha256
                .as_deref()
                .is_some_and(|digest| !valid_digest(digest))
            || self
                .projection_bytes
                .is_some_and(|size| size == 0 || size > PROJECTION_BYTES as u64)
            || matches!(self.phase, CapturePhase::Complete) && self.has_trace != Some(true)
            || matches!(self.phase, CapturePhase::Complete) && self.exit_code != Some(0)
            || matches!(self.phase, CapturePhase::ProjectionPending)
                && self.has_trace == Some(true)
                && self.exit_code != Some(0)
            || matches!(self.phase, CapturePhase::ProjectionPending)
                && self.has_trace == Some(false)
                && !matches!(self.exit_code, Some(0 | 3))
            || matches!(self.phase, CapturePhase::Unavailable)
                && (self.has_trace != Some(false) || !matches!(self.exit_code, Some(0 | 3)))
        {
            return Err(HostError::Ownership);
        }
        Ok(())
    }

    pub(in crate::launch) fn prepared(binding: CaptureBinding, attempt: i64) -> Self {
        Self {
            schema: 3,
            binding,
            attempt,
            create_requests: 0,
            start_requests: 0,
            phase: CapturePhase::Prepared,
            exec_id: None,
            exit_code: None,
            projection_sha256: None,
            projection_bytes: None,
            has_trace: None,
        }
    }

    pub(in crate::launch) fn next_create_request(&self) -> Result<Self, HostError> {
        if self.phase != CapturePhase::Prepared || self.create_requests >= 3 {
            return Err(HostError::Ownership);
        }
        let mut next = self.clone();
        next.create_requests += 1;
        Ok(next)
    }

    pub(in crate::launch) fn created(&self, exec_id: String) -> Result<Self, HostError> {
        if self.phase != CapturePhase::Prepared || !valid_id(&exec_id) {
            return Err(HostError::Ownership);
        }
        let mut next = self.clone();
        next.phase = CapturePhase::Created;
        next.exec_id = Some(exec_id);
        Ok(next)
    }

    pub(in crate::launch) fn next_start_request(&self) -> Result<Self, HostError> {
        if !matches!(
            self.phase,
            CapturePhase::Created | CapturePhase::StartRequested
        ) || self.start_requests >= 3
        {
            return Err(HostError::Ownership);
        }
        let mut next = self.clone();
        next.phase = CapturePhase::StartRequested;
        next.start_requests += 1;
        Ok(next)
    }

    pub(in crate::launch) fn terminal(&self, exit_code: i64) -> Result<Self, HostError> {
        if self.phase != CapturePhase::StartRequested || !(0..=255).contains(&exit_code) {
            return Err(HostError::Ownership);
        }
        let mut next = self.clone();
        next.phase = CapturePhase::Terminal;
        next.exit_code = Some(exit_code);
        Ok(next)
    }

    pub(in crate::launch) fn retry_after_terminal(&self) -> Result<Self, HostError> {
        if !matches!(
            self.phase,
            CapturePhase::Terminal | CapturePhase::ProjectionPending
        ) {
            return Err(HostError::Ownership);
        }
        let next = Self::prepared(
            self.binding.clone(),
            self.attempt.checked_add(1).ok_or(HostError::Ownership)?,
        );
        next.validate(&self.binding)?;
        Ok(next)
    }

    pub(in crate::launch) fn projection_pending(
        &self,
        digest: [u8; 32],
        size: usize,
        has_trace: bool,
    ) -> Result<Self, HostError> {
        if self.phase != CapturePhase::Terminal
            || has_trace && self.exit_code != Some(0)
            || !has_trace && !matches!(self.exit_code, Some(0 | 3))
            || size == 0
            || size > PROJECTION_BYTES
        {
            return Err(HostError::Ownership);
        }
        let mut next = self.clone();
        next.phase = CapturePhase::ProjectionPending;
        next.projection_sha256 = Some(lower_hex(&digest));
        next.projection_bytes = Some(size as u64);
        next.has_trace = Some(has_trace);
        Ok(next)
    }

    pub(in crate::launch) fn projected(&self) -> Result<Self, HostError> {
        if self.phase != CapturePhase::ProjectionPending {
            return Err(HostError::Ownership);
        }
        let mut next = self.clone();
        next.phase = if self.has_trace == Some(true) {
            CapturePhase::Complete
        } else {
            CapturePhase::Unavailable
        };
        Ok(next)
    }
}

fn valid_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn lower_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    value
}
