//! Bounded host-control messages. No credential material is sent.

use serde::{Deserialize, Serialize};

use crate::config::{ConnectionId, RepositoryConnection};
use crate::readiness::ResourceProbeReport;

pub(super) const MAX_REQUEST_BYTES: usize = 65_536;
pub(super) const MAX_RESPONSE_BYTES: usize = 4096;
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
    ResourceProbe,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) value: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) resource_probe: Option<ResourceProbeReport>,
}

impl Response {
    pub(super) const fn boolean(value: Option<bool>) -> Self {
        Self {
            value,
            resource_probe: None,
        }
    }

    pub(super) fn probe(report: Option<ResourceProbeReport>) -> Self {
        Self {
            value: None,
            resource_probe: report,
        }
    }
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
        let bytes = serde_json::to_vec(&self).map_err(|_| ())?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(());
        }
        Ok(bytes)
    }

    pub(super) fn decode(bytes: &[u8]) -> Result<Self, ()> {
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(());
        }
        serde_json::from_slice(bytes).map_err(|_| ())
    }
}
