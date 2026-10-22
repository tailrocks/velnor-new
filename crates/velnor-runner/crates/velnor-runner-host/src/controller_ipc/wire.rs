//! Bounded host-control messages. No credential material is sent.

use serde::{Deserialize, Serialize};

use crate::config::{ConnectionId, RepositoryConnection};
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

#[cfg(test)]
mod tests {
    use super::{DoctorProbeReply, Response};
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct LegacyDoctorProbeReply {
        request_id: Option<i64>,
        state: String,
        report: Option<serde_json::Value>,
    }

    #[test]
    fn doctor_readiness_stays_outside_the_legacy_reply_shape() {
        let response = Response {
            value: None,
            doctor_probe: Some(DoctorProbeReply {
                request_id: Some(17),
                state: "completed".to_owned(),
                report: Some(serde_json::json!({"cpu_count": 8})),
            }),
            readiness: Some(crate::readiness::Readiness::Ready),
        };
        let encoded = serde_json::to_value(response).expect("response serializes");
        let legacy: LegacyDoctorProbeReply =
            serde_json::from_value(encoded["doctor_probe"].clone())
                .expect("legacy doctor client accepts the unchanged nested reply");
        assert_eq!(legacy.request_id, Some(17));
        assert_eq!(legacy.state, "completed");
        assert_eq!(legacy.report, Some(serde_json::json!({"cpu_count": 8})));
        assert_eq!(encoded["readiness"], "ready");
    }

    #[test]
    fn new_client_accepts_an_old_doctor_response_without_readiness() {
        let response: Response = serde_json::from_value(serde_json::json!({
            "value": null,
            "doctor_probe": {
                "request_id": 17,
                "state": "completed",
                "report": {"cpu_count": 8}
            }
        }))
        .expect("new client accepts the old response");
        assert!(response.readiness.is_none());
        let reply = response.doctor_probe.expect("old doctor reply is present");
        assert_eq!(reply.request_id, Some(17));
        assert_eq!(reply.state, "completed");
        assert_eq!(reply.report, Some(serde_json::json!({"cpu_count": 8})));
    }
}
