// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::config::ConnectionId;
use crate::error::HostError;

use super::ConnectionJournal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AssignmentKey {
    AssignedRequest { request_id: i64 },

    Remote { kind: String, subject: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AssignmentIdentity {
    connection_id: ConnectionId,
    scale_set_id: i64,
    key: AssignmentKey,
}

impl AssignmentIdentity {
    pub(crate) fn new(
        connection_id: &ConnectionId,
        scale_set_id: i64,
        key: AssignmentKey,
    ) -> Result<Self, HostError> {
        if scale_set_id <= 0 || !valid_key(&key) {
            return Err(HostError::Journal);
        }
        Ok(Self {
            connection_id: connection_id.clone(),
            scale_set_id,
            key,
        })
    }

    pub(crate) fn connection_id(&self) -> &ConnectionId {
        &self.connection_id
    }

    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    pub(crate) fn key(&self) -> &AssignmentKey {
        &self.key
    }

    pub(crate) fn request_id(&self) -> Option<i64> {
        match &self.key {
            AssignmentKey::AssignedRequest { request_id } => Some(*request_id),
            AssignmentKey::Remote { .. } => None,
        }
    }

    pub(crate) fn storage_key(&self) -> String {
        let prefix = format!("v1|{}|{}|", self.connection_id.as_str(), self.scale_set_id);
        match &self.key {
            AssignmentKey::AssignedRequest { request_id } => {
                format!("{prefix}request|{request_id}")
            }

            AssignmentKey::Remote { kind, subject } => {
                format!(
                    "{prefix}remote|{}:{kind}|{}:{subject}",
                    kind.len(),
                    subject.len()
                )
            }
        }
    }
}

impl ConnectionJournal {
    pub(crate) async fn assignment_identity(
        &self,
        key: AssignmentKey,
    ) -> Result<AssignmentIdentity, HostError> {
        let scale_set = self.scale_set().await?.ok_or(HostError::Journal)?;
        AssignmentIdentity::new(&self.connection_id, scale_set.id, key)
    }
}

fn valid_key(key: &AssignmentKey) -> bool {
    match key {
        AssignmentKey::AssignedRequest { request_id } => *request_id > 0,

        AssignmentKey::Remote { kind, subject } => {
            valid_text(kind, 128) && valid_text(subject, 1024)
        }
    }
}

fn valid_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && !value.chars().any(|ch| matches!(ch, '\'' | '"' | '\0'))
}
