// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::config::ConnectionId;
use crate::journal::capacity_header::HeaderAllocation;

const NONCE_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionCreateState {
    Prepared,

    Requested,

    Uncertain,

    Bound,

    Retired,
}

impl SessionCreateState {
    pub(super) fn parse(value: &str) -> Result<Self, crate::error::HostError> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "requested" => Ok(Self::Requested),
            "uncertain" => Ok(Self::Uncertain),
            "bound" => Ok(Self::Bound),
            "retired" => Ok(Self::Retired),
            _ => Err(crate::error::HostError::Journal),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionCreateRecord {
    pub(super) connection_id: ConnectionId,
    pub(super) scale_set_id: i64,
    pub(super) local_nonce: String,
    pub(super) state: SessionCreateState,
    pub(super) session_id: Option<String>,
    pub(super) intent_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoundConnectionSession {
    pub(super) intent_id: i64,
    pub(super) allocation: HeaderAllocation,
}

impl BoundConnectionSession {
    #[must_use]
    pub(crate) const fn intent_id(&self) -> i64 {
        self.intent_id
    }

    #[must_use]
    pub(crate) const fn allocation(&self) -> &HeaderAllocation {
        &self.allocation
    }
}

impl SessionCreateRecord {
    #[must_use]
    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    #[must_use]
    pub(crate) fn local_nonce(&self) -> &str {
        &self.local_nonce
    }

    #[must_use]
    pub(crate) const fn state(&self) -> SessionCreateState {
        self.state
    }

    #[must_use]
    pub(crate) fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    #[must_use]
    pub(crate) const fn intent_id(&self) -> Option<i64> {
        self.intent_id
    }
}

#[must_use = "a claimed session create must be resolved or kept uncertain"]
pub(crate) struct SessionCreateClaim {
    pub(super) connection_id: ConnectionId,
    pub(super) scale_set_id: i64,
    pub(super) local_nonce: String,
}

impl SessionCreateClaim {
    pub(super) fn new(connection_id: ConnectionId, scale_set_id: i64, local_nonce: String) -> Self {
        Self {
            connection_id,
            scale_set_id,
            local_nonce,
        }
    }
}

pub(super) fn validate_nonce(value: &str) -> Result<(), crate::error::HostError> {
    if value.len() != NONCE_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(crate::error::HostError::Journal);
    }
    Ok(())
}

pub(super) fn validate_session_id(value: &str) -> Result<(), crate::error::HostError> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(crate::error::HostError::Journal);
    }
    Ok(())
}
