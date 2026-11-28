// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use serde::{Deserialize, Serialize};
use std::fmt;

use velnor_model::RUNNER_PROTOCOL_VERSION;

pub const RUNNER_JOB_REQUEST: &str = "RunnerJobRequest";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAgentSession {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub owner_name: String,
    #[serde(default)]
    pub agent: TaskAgentReference,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub broker_session: bool,
}

impl fmt::Debug for TaskAgentSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskAgentSession")
            .field("session_id", &self.session_id)
            .field("owner_name", &self.owner_name)
            .field("agent", &self.agent)
            .field("broker_session", &self.broker_session)
            .finish()
    }
}

impl TaskAgentSession {
    pub fn new(
        owner_name: impl Into<String>,
        agent_id: i64,
        agent_name: impl Into<String>,
    ) -> Self {
        Self {
            session_id: None,
            owner_name: owner_name.into(),
            agent: TaskAgentReference {
                id: agent_id,
                name: agent_name.into(),
                version: RUNNER_PROTOCOL_VERSION.to_owned(),
                os_description: std::env::consts::OS.to_owned(),
            },
            broker_session: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAgentReference {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub os_description: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskAgentMessage {
    #[serde(default)]
    pub message_id: i64,
    pub message_type: String,
    pub body: String,
}

impl fmt::Debug for TaskAgentMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskAgentMessage")
            .field("message_id", &self.message_id)
            .field("message_type", &self.message_type)
            .field("body", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RunnerJobRequestRef {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(alias = "runnerRequestId")]
    pub runner_request_id: String,
    #[serde(default, alias = "shouldAcknowledge")]
    pub should_acknowledge: bool,
    #[serde(default, alias = "runServiceUrl")]
    pub run_service_url: Option<String>,
    #[serde(default, alias = "billingOwnerId")]
    pub billing_owner_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerStatus {
    Online,
    Busy,
    Offline,
}

impl RunnerStatus {
    #[must_use]
    pub const fn as_query_value(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::Busy => "Busy",
            Self::Offline => "Offline",
        }
    }
}
