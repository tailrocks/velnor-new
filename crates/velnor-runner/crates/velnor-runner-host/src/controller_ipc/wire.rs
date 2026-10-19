//! Bounded host-control messages. No credential material is sent.

use serde::{Deserialize, Serialize};

use crate::config::{ConnectionId, RepositoryConnection};

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
}

impl Response {
    pub(super) fn encode(self) -> Result<Vec<u8>, ()> {
        serde_json::to_vec(&self).map_err(|_| ())
    }

    pub(super) fn decode(bytes: &[u8]) -> Result<Self, ()> {
        if bytes.len() > 4096 {
            return Err(());
        }
        serde_json::from_slice(bytes).map_err(|_| ())
    }
}
