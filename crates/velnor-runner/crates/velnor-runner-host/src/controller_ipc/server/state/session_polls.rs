//! Track successful and in-flight polls for configured sessions.

use std::time::{Duration, Instant};

use crate::config::RepositoryConnection;
use crate::error::HostError;
use crate::launch::steady::PollObservation;

use super::ControlState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionPollReceipt {
    pub(in crate::controller_ipc::server) connection_id: String,
    pub(in crate::controller_ipc::server) binding_sha256: String,
    pub(in crate::controller_ipc::server) scale_set_id: i64,
    pub(in crate::controller_ipc::server) session_intent_id: i64,
    pub(in crate::controller_ipc::server) session_id: String,
    pub(in crate::controller_ipc::server) generation: u64,
    pub(in crate::controller_ipc::server) observed_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PendingSessionPoll {
    binding_sha256: String,
    scale_set_id: i64,
    session_intent_id: i64,
    session_id: String,
    sequence: u64,
    pub(super) deadline: Instant,
}

impl ControlState {
    #[cfg(test)]
    pub(crate) fn record_session_poll(
        &self,
        binding: &RepositoryConnection,
        scale_set_id: i64,
        session_intent_id: i64,
        session_id: &str,
    ) -> Result<(), HostError> {
        self.record_session_poll_observation(
            binding,
            scale_set_id,
            session_intent_id,
            session_id,
            None,
        )
    }

    pub(crate) fn record_session_poll_observation(
        &self,
        binding: &RepositoryConnection,
        scale_set_id: i64,
        session_intent_id: i64,
        session_id: &str,
        observation: Option<PollObservation>,
    ) -> Result<(), HostError> {
        let mut coordinator = self
            .session_observation
            .lock()
            .map_err(|_| HostError::Journal)?;
        if scale_set_id <= 0 || session_intent_id <= 0 || session_id.trim().is_empty() {
            return Err(HostError::Identity);
        }
        if !self
            .bindings
            .read()
            .map_err(|_| HostError::Journal)?
            .iter()
            .any(|current| current == binding)
        {
            return Err(HostError::Config);
        }
        let binding_sha256 = binding_sha256(binding)?;
        let connection_id = binding.connection_id.as_str().to_owned();
        let mut polls = self.session_polls.write().map_err(|_| HostError::Journal)?;
        let generation = polls
            .get(&connection_id)
            .map_or(Some(1), |receipt| receipt.generation.checked_add(1))
            .ok_or(HostError::Journal)?;
        polls.insert(
            connection_id.clone(),
            SessionPollReceipt {
                connection_id: connection_id.clone(),
                binding_sha256: binding_sha256.clone(),
                scale_set_id,
                session_intent_id,
                session_id: session_id.to_owned(),
                generation,
                observed_at: Instant::now(),
            },
        );
        let pending = self
            .pending_polls
            .write()
            .map_err(|_| HostError::Journal)?
            .remove(&connection_id);
        coordinator.finish_poll(super::session_observation::PollCompletion {
            connection_id: &connection_id,
            poll_sequence: pending.map(|attempt| attempt.sequence),
            binding_sha256: &binding_sha256,
            scale_set_id,
            session_intent_id,
            session_id,
            poll: observation,
        });
        self.readiness
            .write()
            .map_err(|_| HostError::Journal)?
            .take();
        Ok(())
    }

    pub(crate) fn begin_session_poll(
        &self,
        binding: &RepositoryConnection,
        scale_set_id: i64,
        session_intent_id: i64,
        session_id: &str,
        deadline: Instant,
    ) -> Result<(), HostError> {
        let mut coordinator = self
            .session_observation
            .lock()
            .map_err(|_| HostError::Journal)?;
        let now = Instant::now();
        if scale_set_id <= 0
            || session_intent_id <= 0
            || session_id.trim().is_empty()
            || deadline <= now
            || deadline.duration_since(now) > Duration::from_secs(60)
        {
            return Err(HostError::Identity);
        }
        if !self.matches_bound_session(binding, scale_set_id, session_intent_id, session_id)? {
            return Err(HostError::Identity);
        }
        let connection_id = binding.connection_id.as_str().to_owned();
        let mut pending = self.pending_polls.write().map_err(|_| HostError::Journal)?;
        if pending
            .get(&connection_id)
            .is_some_and(|attempt| Instant::now() <= attempt.deadline)
        {
            return Err(HostError::Ownership);
        }
        let sequence = coordinator.begin_poll()?;
        pending.insert(
            connection_id,
            PendingSessionPoll {
                binding_sha256: binding_sha256(binding)?,
                scale_set_id,
                session_intent_id,
                session_id: session_id.to_owned(),
                sequence,
                deadline,
            },
        );
        Ok(())
    }

    fn matches_bound_session(
        &self,
        binding: &RepositoryConnection,
        scale_set_id: i64,
        session_intent_id: i64,
        session_id: &str,
    ) -> Result<bool, HostError> {
        let expected_digest = binding_sha256(binding)?;
        let bindings_match = self
            .bindings
            .read()
            .map_err(|_| HostError::Journal)?
            .iter()
            .any(|current| current == binding);
        let polls = self.session_polls.read().map_err(|_| HostError::Journal)?;
        let prior_matches = polls
            .get(binding.connection_id.as_str())
            .is_none_or(|receipt| {
                receipt.scale_set_id == scale_set_id
                    && receipt.session_intent_id == session_intent_id
                    && receipt.session_id == session_id
                    && receipt.binding_sha256 == expected_digest
            });
        Ok(bindings_match && prior_matches)
    }

    pub(crate) fn invalidate_session_poll(&self, connection_id: &str) -> Result<(), HostError> {
        let mut coordinator = self
            .session_observation
            .lock()
            .map_err(|_| HostError::Journal)?;
        coordinator.invalidate_connection(connection_id);
        let setup_removed = coordinator.setup_receipts.remove(connection_id).is_some();
        let removed = self
            .session_polls
            .write()
            .map_err(|_| HostError::Journal)?
            .remove(connection_id);
        self.pending_polls
            .write()
            .map_err(|_| HostError::Journal)?
            .remove(connection_id);
        if removed.is_some() || setup_removed {
            self.clear_readiness()?;
        }
        Ok(())
    }

    pub(crate) fn invalidate_if_session_changed(
        &self,
        binding: &RepositoryConnection,
        scale_set_id: i64,
        session_intent_id: i64,
        session_id: &str,
    ) -> Result<(), HostError> {
        let mut coordinator = self
            .session_observation
            .lock()
            .map_err(|_| HostError::Journal)?;
        let binding_sha256 = binding_sha256(binding)?;
        let mut polls = self.session_polls.write().map_err(|_| HostError::Journal)?;
        let connection_id = binding.connection_id.as_str();
        if polls.get(connection_id).is_some_and(|receipt| {
            receipt.binding_sha256 != binding_sha256
                || receipt.scale_set_id != scale_set_id
                || receipt.session_intent_id != session_intent_id
                || receipt.session_id != session_id
        }) {
            coordinator.invalidate_connection(connection_id);
            coordinator.setup_receipts.remove(connection_id);
            polls.remove(connection_id);
            self.pending_polls
                .write()
                .map_err(|_| HostError::Journal)?
                .remove(connection_id);
            self.clear_readiness()?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn invalidate_session_polls(&self) -> Result<(), HostError> {
        let mut coordinator = self
            .session_observation
            .lock()
            .map_err(|_| HostError::Journal)?;
        coordinator.invalidate_all();
        coordinator.setup_receipts.clear();
        self.session_polls
            .write()
            .map_err(|_| HostError::Journal)?
            .clear();
        self.pending_polls
            .write()
            .map_err(|_| HostError::Journal)?
            .clear();
        self.clear_readiness()?;
        Ok(())
    }

    pub(crate) fn complete_session_poll_receipts(
        &self,
    ) -> Result<Option<Vec<SessionPollReceipt>>, HostError> {
        let bindings = self.bindings.read().map_err(|_| HostError::Journal)?;
        if bindings.is_empty() {
            return Ok(None);
        }
        let polls = self.session_polls.read().map_err(|_| HostError::Journal)?;
        let pending = self.pending_polls.read().map_err(|_| HostError::Journal)?;
        let mut receipts = Vec::with_capacity(bindings.len());
        for binding in bindings.iter() {
            let Some(receipt) = polls.get(binding.connection_id.as_str()) else {
                return Ok(None);
            };
            if receipt.connection_id != binding.connection_id.as_str()
                || receipt.binding_sha256 != binding_sha256(binding)?
                || receipt.scale_set_id <= 0
                || receipt.session_intent_id <= 0
                || receipt.session_id.trim().is_empty()
                || receipt.generation == 0
            {
                return Ok(None);
            }
            let attempt = pending.get(binding.connection_id.as_str());
            let is_pending = attempt.is_some_and(|attempt| {
                attempt.binding_sha256 == receipt.binding_sha256
                    && attempt.scale_set_id == receipt.scale_set_id
                    && attempt.session_intent_id == receipt.session_intent_id
                    && attempt.session_id == receipt.session_id
                    && Instant::now() <= attempt.deadline
            });
            if (attempt.is_some() && !is_pending)
                || (attempt.is_none()
                    && Instant::now()
                        .checked_duration_since(receipt.observed_at)
                        .is_none_or(|age| age > Duration::from_secs(5)))
            {
                return Ok(None);
            }
            receipts.push(receipt.clone());
        }
        if polls.len() != receipts.len() {
            return Ok(None);
        }
        Ok(Some(receipts))
    }
}

impl SessionPollReceipt {
    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    pub(crate) const fn session_intent_id(&self) -> i64 {
        self.session_intent_id
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }
}

pub(super) fn binding_sha256(binding: &RepositoryConnection) -> Result<String, HostError> {
    use sha2::Digest;

    let bytes = serde_json::to_vec(binding).map_err(|_| HostError::Identity)?;
    let mut result = String::with_capacity(64);
    for byte in sha2::Sha256::digest(bytes) {
        use std::fmt::Write as _;
        write!(result, "{byte:02x}").map_err(|_| HostError::Identity)?;
    }
    Ok(result)
}
