//! Keep controller-owned IPC evidence in one private state object.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::config::{HostConfig, RepositoryConnection};
use crate::controller_ipc::wire::{SessionObservationReply, SessionObservationStatus};
use crate::error::HostError;
use crate::launch::ReadinessSample;

mod readiness;
pub(crate) use readiness::ControllerReadinessProof;
mod session_observation;
use session_observation::{SessionObservationCoordinator, SessionObservationTicket, TicketOutcome};
mod setup_receipt;
pub(crate) use setup_receipt::SetupReceiptInput;
mod connection_census;
mod session_polls;
pub(crate) use session_polls::SessionPollReceipt;
use session_polls::binding_sha256;

const DOCTOR_REPORT_MAX_AGE: Duration = Duration::from_secs(30);

/// Live config bindings read by the private request listener.
#[derive(Debug, Clone)]
pub(crate) struct ControlState {
    config: Arc<RwLock<Option<HostConfig>>>,
    pub(super) bindings: Arc<RwLock<Vec<RepositoryConnection>>>,
    session_polls: Arc<RwLock<BTreeMap<String, session_polls::SessionPollReceipt>>>,
    pending_polls: Arc<RwLock<BTreeMap<String, session_polls::PendingSessionPoll>>>,
    session_observation: Arc<Mutex<SessionObservationCoordinator>>,
    readiness: Arc<RwLock<Option<readiness::ControllerReadinessProof>>>,
    readiness_sample: Arc<RwLock<Option<ReadinessSample>>>,
    admission_ready: Arc<AtomicBool>,
    doctor_report: Arc<RwLock<Option<(String, serde_json::Value, Instant)>>>,
}

struct ObservationStart<'a> {
    connection_id: &'a str,
    nonce: &'a str,
    engine_id: &'a str,
    docker_root_digest: &'a str,
    now: Instant,
    deadline: Instant,
    expires_at: Instant,
    setup_revision: Option<u64>,
}

impl ControlState {
    #[cfg(test)]
    pub(super) fn empty() -> Self {
        Self {
            config: Arc::new(RwLock::new(None)),
            bindings: Arc::new(RwLock::new(Vec::new())),
            session_polls: Arc::new(RwLock::new(BTreeMap::new())),
            pending_polls: Arc::new(RwLock::new(BTreeMap::new())),
            session_observation: Arc::new(Mutex::new(SessionObservationCoordinator::default())),
            readiness: Arc::new(RwLock::new(None)),
            readiness_sample: Arc::new(RwLock::new(None)),
            admission_ready: Arc::new(AtomicBool::new(false)),
            doctor_report: Arc::new(RwLock::new(None)),
        }
    }

    pub(crate) fn new(config: &HostConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(Some(config.clone()))),
            bindings: Arc::new(RwLock::new(config.connections().to_vec())),
            session_polls: Arc::new(RwLock::new(BTreeMap::new())),
            pending_polls: Arc::new(RwLock::new(BTreeMap::new())),
            session_observation: Arc::new(Mutex::new(SessionObservationCoordinator::default())),
            readiness: Arc::new(RwLock::new(None)),
            readiness_sample: Arc::new(RwLock::new(None)),
            admission_ready: Arc::new(AtomicBool::new(false)),
            doctor_report: Arc::new(RwLock::new(None)),
        }
    }

    #[cfg(test)]
    pub(crate) fn replace_config(&self, config: &HostConfig) -> Result<(), HostError> {
        let mut observation = self
            .session_observation
            .lock()
            .map_err(|_| HostError::Journal)?;
        observation.replace_config(config.connections())?;
        *self.config.write().map_err(|_| HostError::Journal)? = Some(config.clone());
        let mut bindings = self.bindings.write().map_err(|_| HostError::Journal)?;
        *bindings = config.connections().to_vec();
        observation.setup_receipts.clear();
        self.session_polls
            .write()
            .map_err(|_| HostError::Journal)?
            .clear();
        self.pending_polls
            .write()
            .map_err(|_| HostError::Journal)?
            .clear();
        self.clear_readiness()?;
        self.clear_readiness_sample()?;
        Ok(())
    }

    pub(crate) fn matches_config(&self, config: &HostConfig) -> Result<bool, HostError> {
        Ok(self
            .config
            .read()
            .map_err(|_| HostError::Journal)?
            .as_ref()
            .is_some_and(|current| current == config))
    }

    pub(crate) fn set_admission_ready(&self, ready: bool) {
        let Ok(mut observation) = self.session_observation.lock() else {
            self.admission_ready.store(false, Ordering::Release);
            return;
        };
        self.admission_ready.store(ready, Ordering::Release);
        if !ready {
            observation.invalidate_all();
            observation.setup_receipts.clear();
            if let Err(error) = self.clear_readiness() {
                eprintln!("The controller could not clear readiness evidence. Error: {error}.");
            }
            if let Err(error) = self.clear_readiness_sample() {
                eprintln!("The controller could not clear the readiness sample. Error: {error}.");
            }
        }
    }

    pub(crate) fn admission_ready(&self) -> bool {
        self.admission_ready.load(Ordering::Acquire)
    }

    pub(crate) fn contains_connection(
        &self,
        binding: &RepositoryConnection,
    ) -> Result<bool, HostError> {
        let bindings = self.bindings.read().map_err(|_| HostError::Journal)?;
        Ok(bindings
            .iter()
            .any(|candidate| candidate.connection_id == binding.connection_id))
    }

    pub(crate) fn save_doctor_report(
        &self,
        nonce: &str,
        report: serde_json::Value,
    ) -> Result<(), HostError> {
        *self.doctor_report.write().map_err(|_| HostError::Journal)? =
            Some((nonce.to_owned(), report, Instant::now()));
        Ok(())
    }

    pub(super) fn doctor_report(
        &self,
        nonce: &str,
    ) -> Result<Option<serde_json::Value>, HostError> {
        let report = self.doctor_report.read().map_err(|_| HostError::Journal)?;
        Ok(report.as_ref().and_then(|(key, value, measured_at)| {
            (key == nonce
                && Instant::now()
                    .checked_duration_since(*measured_at)
                    .is_some_and(|age| age <= DOCTOR_REPORT_MAX_AGE))
            .then(|| value.clone())
        }))
    }
}

impl ControlState {
    pub(super) fn start_session_observation(
        &self,
        connection_id: &str,
        nonce: &str,
        timeout: Duration,
        engine_id: &str,
        docker_root_digest: &str,
    ) -> Result<SessionObservationReply, HostError> {
        if !session_observation::valid_nonce(nonce)
            || timeout.is_zero()
            || timeout > session_observation::REQUEST_MAX_TIMEOUT
            || !session_observation::valid_engine(engine_id)
            || !session_observation::valid_digest(docker_root_digest)
        {
            return Err(HostError::Frame);
        }
        let now = Instant::now();
        let deadline = now.checked_add(timeout).ok_or(HostError::Identity)?;
        let expires_at = now
            .checked_add(session_observation::TICKET_TTL)
            .ok_or(HostError::Identity)?;
        let mut coordinator = self
            .session_observation
            .lock()
            .map_err(|_| HostError::Journal)?;
        coordinator.remove_expired(now);
        if !self.admission_ready() {
            return Ok(session_observation::reply(
                SessionObservationStatus::Stale,
                None,
                None,
            ));
        }
        let request = ObservationStart {
            connection_id,
            nonce,
            engine_id,
            docker_root_digest,
            now,
            deadline,
            expires_at,
            setup_revision: None,
        };
        if let Some(reply) = self.start_existing_observation(&mut coordinator, &request)? {
            return Ok(reply);
        }
        self.start_new_observation(&mut coordinator, &request)
    }

    fn start_existing_observation(
        &self,
        coordinator: &mut SessionObservationCoordinator,
        request: &ObservationStart<'_>,
    ) -> Result<Option<SessionObservationReply>, HostError> {
        if let Some(ticket) = coordinator.tickets.get(request.connection_id) {
            if ticket.nonce == request.nonce {
                let matches = self.ticket_matches_current(
                    coordinator,
                    ticket,
                    request.connection_id,
                    request.engine_id,
                    request.docker_root_digest,
                    coordinator.config_generation,
                )?;
                let ticket = coordinator
                    .tickets
                    .get_mut(request.connection_id)
                    .ok_or(HostError::Journal)?;
                if !matches {
                    ticket.outcome = TicketOutcome::Stale;
                }
                if ticket.setup_revision != request.setup_revision {
                    ticket.outcome = TicketOutcome::Stale;
                }
                return Ok(Some(ticket.reply(request.now)));
            }
            if matches!(&ticket.outcome, TicketOutcome::Pending) && request.now < ticket.deadline {
                return Ok(Some(session_observation::reply(
                    SessionObservationStatus::Busy,
                    None,
                    None,
                )));
            }
        }
        Ok(None)
    }

    fn start_new_observation(
        &self,
        coordinator: &mut SessionObservationCoordinator,
        request: &ObservationStart<'_>,
    ) -> Result<SessionObservationReply, HostError> {
        coordinator.tickets.remove(request.connection_id);
        let Some(binding) = self.current_binding(request.connection_id)? else {
            return Ok(session_observation::reply(
                SessionObservationStatus::Missing,
                None,
                None,
            ));
        };
        let digest = binding_sha256(&binding)?;
        if self.poll_pending(request.connection_id, request.now)? {
            return Ok(session_observation::reply(
                SessionObservationStatus::Busy,
                None,
                None,
            ));
        }
        let Some(receipt) = self.current_receipt(request.connection_id, &digest, request.now)?
        else {
            return Ok(session_observation::reply(
                SessionObservationStatus::Stale,
                None,
                None,
            ));
        };
        let ticket = SessionObservationTicket {
            nonce: request.nonce.to_owned(),
            binding_sha256: digest,
            config_generation: coordinator.config_generation,
            engine_id: request.engine_id.to_owned(),
            docker_root_digest: request.docker_root_digest.to_owned(),
            scale_set_id: receipt.scale_set_id,
            session_intent_id: receipt.session_intent_id,
            session_id: receipt.session_id.clone(),
            baseline_poll_sequence: coordinator.poll_sequence(),
            deadline: request.deadline,
            expires_at: request.expires_at,
            setup_revision: request.setup_revision,
            outcome: TicketOutcome::Pending,
        };
        coordinator
            .tickets
            .insert(request.connection_id.to_owned(), ticket);
        Ok(session_observation::reply(
            SessionObservationStatus::Pending,
            None,
            None,
        ))
    }

    pub(super) fn session_observation_status(
        &self,
        connection_id: &str,
        nonce: &str,
        engine_id: &str,
        docker_root_digest: &str,
    ) -> Result<SessionObservationReply, HostError> {
        if !session_observation::valid_nonce(nonce)
            || !session_observation::valid_engine(engine_id)
            || !session_observation::valid_digest(docker_root_digest)
        {
            return Err(HostError::Frame);
        }
        let now = Instant::now();
        let mut coordinator = self
            .session_observation
            .lock()
            .map_err(|_| HostError::Journal)?;
        coordinator.remove_expired(now);
        if !self.admission_ready() {
            coordinator.invalidate_connection(connection_id);
        }
        let Some(ticket) = coordinator.tickets.get(connection_id) else {
            return Ok(session_observation::reply(
                SessionObservationStatus::Missing,
                None,
                None,
            ));
        };
        if ticket.nonce != nonce {
            return Ok(session_observation::reply(
                SessionObservationStatus::Missing,
                None,
                None,
            ));
        }
        let matches = self.ticket_matches_current(
            &coordinator,
            ticket,
            connection_id,
            engine_id,
            docker_root_digest,
            coordinator.config_generation,
        )?;
        let ticket = coordinator
            .tickets
            .get_mut(connection_id)
            .ok_or(HostError::Journal)?;
        if !matches {
            ticket.outcome = TicketOutcome::Stale;
        }
        Ok(ticket.reply(now))
    }

    pub(crate) fn invalidate_session_observation(
        &self,
        connection_id: &str,
    ) -> Result<(), HostError> {
        let mut observation = self
            .session_observation
            .lock()
            .map_err(|_| HostError::Journal)?;
        observation.invalidate_connection(connection_id);
        observation.setup_receipts.remove(connection_id);
        Ok(())
    }
}
