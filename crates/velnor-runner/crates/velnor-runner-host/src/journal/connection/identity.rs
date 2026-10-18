//! Typed connection and Scale Set assignment keys.

use crate::config::ConnectionId;
use crate::error::HostError;

use super::ConnectionJournal;

/// Assignment kind used by reservations and their launch intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AssignmentKey {
    /// One acquired GitHub request. Replays use the stable RequestId.
    AssignedRequest { request_id: i64 },
    /// One message that reports assigned population without a RequestId.
    PopulationMessage { message_id: i64 },
    /// One Scale Set session identity.
    Session { name: String },
    /// One other remote intent with an explicit kind and subject.
    Remote { kind: String, subject: String },
}

/// Immutable assignment identity shared by admission and launch journal calls.
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
            _ => None,
        }
    }

    pub(crate) fn storage_key(&self) -> String {
        let prefix = format!("v1|{}|{}|", self.connection_id.as_str(), self.scale_set_id);
        match &self.key {
            AssignmentKey::AssignedRequest { request_id } => {
                format!("{prefix}request|{request_id}")
            }
            AssignmentKey::PopulationMessage { message_id } => {
                format!("{prefix}message|{message_id}")
            }
            AssignmentKey::Session { name } => format!("{prefix}session|{name}"),
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
    /// Build an assignment key from this connection's verified Scale Set binding.
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
        AssignmentKey::PopulationMessage { message_id } => *message_id >= 0,
        AssignmentKey::Session { name } => valid_text(name, 128),
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
