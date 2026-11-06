// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::config::{ConnectionId, RepositoryConnection};
use crate::launch::steady::PollObservation;
use crate::readiness::Readiness;

pub(super) const MAX_REQUEST_BYTES: usize = 65_536;
pub(super) const SOCKET_NAME: &str = "controller.sock";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) enum Request {
    DrainStatus {
        target: Target,
        token: String,
    },
    Reactivate {
        binding: RepositoryConnection,
        token: String,
    },
    Detached {
        binding: RepositoryConnection,
        token: String,
    },
    DoctorProbeStart {
        nonce: String,
    },
    DoctorProbeStatus {
        nonce: String,
    },
    ControllerReadiness {
        config_sha256: String,
        engine_id: String,
        docker_root_digest: String,
    },
    SessionObservationStart {
        connection_id: ConnectionId,
        nonce: String,
        timeout_ms: u64,
    },
    SessionObservationStatus {
        connection_id: ConnectionId,
        nonce: String,
    },
    ConnectionSetupStatus {
        connection_id: ConnectionId,
    },
    ConnectionCensusStart {
        connection_id: ConnectionId,
        nonce: String,
        timeout_ms: u64,
    },
    ConnectionCensusStatus {
        connection_id: ConnectionId,
        nonce: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) enum Target {
    Host,
    Connection {
        connection_id: ConnectionId,
        binding: Box<RepositoryConnection>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Response {
    pub(super) value: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) doctor_probe: Option<DoctorProbeReply>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) readiness: Option<Readiness>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) session_observation: Option<SessionObservationReply>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) setup_receipt: Option<SetupReceiptReply>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) connection_census: Option<ConnectionCensusReply>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SetupReceiptStatus {
    Pending,
    Ready,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RunnerGroupSnapshot {
    pub(super) id: i64,
    pub(super) name: String,
    pub(super) is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SelectedSetSnapshot {
    pub(super) id: i64,
    pub(super) name: String,
    pub(super) runner_group_id: i64,
    pub(super) disable_update: bool,
    pub(super) labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConnectionSetupSnapshot {
    pub(super) revision: u64,
    pub(super) captured_at_unix_ms: u64,
    pub(super) runner_groups: Vec<RunnerGroupSnapshot>,
    pub(super) selected_set: SelectedSetSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetupReceiptReply {
    pub(super) status: SetupReceiptStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) setup: Option<ConnectionSetupSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConnectionCensusReply {
    pub(super) setup_status: SetupReceiptStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) setup: Option<ConnectionSetupSnapshot>,
    pub(super) observation: SessionObservationReply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SessionObservationStatus {
    Pending,
    Complete,
    Unavailable,
    Stale,
    TimedOut,
    Busy,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SessionObservationReply {
    pub(super) status: SessionObservationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) poll: Option<PollObservation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) observed_at_unix_ms: Option<u64>,
}

impl SessionObservationReply {
    pub(super) fn stale() -> Self {
        Self {
            status: SessionObservationStatus::Stale,
            poll: None,
            observed_at_unix_ms: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DoctorProbeReply {
    pub(super) request_id: Option<i64>,
    pub(super) state: String,
    pub(super) report: Option<serde_json::Value>,
}

impl Request {
    pub(super) fn encode(&self) -> Result<Vec<u8>, ()> {
        let bytes = serde_json::to_vec(self).map_err(|_| ())?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(());
        }
        Ok(bytes)
    }

    pub(super) const fn response_limit(&self) -> usize {
        match self {
            Self::ConnectionSetupStatus { .. }
            | Self::ConnectionCensusStart { .. }
            | Self::ConnectionCensusStatus { .. } => crate::ipc::MAX_FRAME,
            _ => 4096,
        }
    }
}

impl Response {
    pub(super) fn encode(self) -> Result<Vec<u8>, ()> {
        serde_json::to_vec(&self).map_err(|_| ())
    }

    pub(super) fn decode_with_limit(bytes: &[u8], limit: usize) -> Result<Self, ()> {
        if bytes.len() > limit {
            return Err(());
        }
        serde_json::from_slice(bytes).map_err(|_| ())
    }
}
