//! Bounded host-control messages. No credential material is sent.

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

    #[cfg(test)]
    pub(super) fn decode(bytes: &[u8]) -> Result<Self, ()> {
        Self::decode_with_limit(bytes, 4096)
    }

    pub(super) fn decode_with_limit(bytes: &[u8], limit: usize) -> Result<Self, ()> {
        if bytes.len() > limit {
            return Err(());
        }
        serde_json::from_slice(bytes).map_err(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConnectionSetupSnapshot, DoctorProbeReply, Request, Response, RunnerGroupSnapshot,
        SelectedSetSnapshot, SessionObservationReply, SessionObservationStatus, SetupReceiptReply,
        SetupReceiptStatus,
    };
    use crate::launch::steady::{PollObservation, PollStatistics};
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
            session_observation: None,
            setup_receipt: None,
            connection_census: None,
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

    #[test]
    fn observation_response_serializes_only_sanitized_poll_counts() {
        let response = Response {
            value: None,
            doctor_probe: None,
            readiness: None,
            session_observation: Some(SessionObservationReply {
                status: SessionObservationStatus::Complete,
                poll: Some(PollObservation::Complete {
                    available_events: 2,
                    statistics: Some(PollStatistics {
                        available_jobs: 11,
                        acquired_jobs: 12,
                        assigned_jobs: 13,
                        running_jobs: 14,
                        registered_runners: 15,
                        busy_runners: 16,
                        idle_runners: 17,
                    }),
                }),
                observed_at_unix_ms: Some(1234),
            }),
            setup_receipt: None,
            connection_census: None,
        };
        let encoded = serde_json::to_string(&response).expect("response serializes");
        assert!(encoded.contains("available_events"));
        assert!(encoded.contains("available_jobs"));
        assert!(!encoded.contains("session_id"));
        assert!(!encoded.contains("message_id"));
        assert!(!encoded.contains("raw_body"));
    }

    #[test]
    fn setup_snapshot_uses_the_extended_response_limit() {
        let response = Response {
            value: None,
            doctor_probe: None,
            readiness: None,
            session_observation: None,
            setup_receipt: Some(SetupReceiptReply {
                status: SetupReceiptStatus::Ready,
                setup: Some(ConnectionSetupSnapshot {
                    revision: 1,
                    captured_at_unix_ms: 1,
                    runner_groups: vec![RunnerGroupSnapshot {
                        id: 7,
                        name: "g".repeat(6_000),
                        is_default: true,
                    }],
                    selected_set: SelectedSetSnapshot {
                        id: 17,
                        name: "set".to_owned(),
                        runner_group_id: 7,
                        disable_update: true,
                        labels: vec!["linux".to_owned()],
                    },
                }),
            }),
            connection_census: None,
        };
        let bytes = serde_json::to_vec(&response).expect("response serializes");
        let connection_id =
            crate::config::ConnectionId::parse("owner").expect("test connection ID is valid");
        let setup_request = Request::ConnectionSetupStatus { connection_id };
        assert!(bytes.len() > 4096);
        assert!(bytes.len() <= crate::ipc::MAX_FRAME);
        assert!(Response::decode(&bytes).is_err());
        assert!(Response::decode_with_limit(&bytes, setup_request.response_limit()).is_ok());
        assert_eq!(setup_request.response_limit(), crate::ipc::MAX_FRAME);
        assert_eq!(
            Request::DoctorProbeStatus {
                nonce: "a".repeat(32)
            }
            .response_limit(),
            4096
        );
    }

    #[test]
    fn setup_names_are_json_escaped_without_session_identity() {
        let response = Response {
            value: None,
            doctor_probe: None,
            readiness: None,
            session_observation: None,
            setup_receipt: Some(SetupReceiptReply {
                status: SetupReceiptStatus::Ready,
                setup: Some(ConnectionSetupSnapshot {
                    revision: 2,
                    captured_at_unix_ms: 3,
                    runner_groups: vec![RunnerGroupSnapshot {
                        id: 7,
                        name: "group\nname".to_owned(),
                        is_default: true,
                    }],
                    selected_set: SelectedSetSnapshot {
                        id: 17,
                        name: "set\nname".to_owned(),
                        runner_group_id: 7,
                        disable_update: false,
                        labels: vec!["linux\nlarge".to_owned()],
                    },
                }),
            }),
            connection_census: None,
        };
        let encoded = serde_json::to_string(&response).expect("response serializes");
        assert!(encoded.contains("group\\nname"));
        assert!(encoded.contains("set\\nname"));
        assert!(encoded.contains("linux\\nlarge"));
        assert!(!encoded.contains("\nsession_id"));
        assert!(!encoded.contains("session_id"));
        assert!(!encoded.contains("token"));
    }
}
