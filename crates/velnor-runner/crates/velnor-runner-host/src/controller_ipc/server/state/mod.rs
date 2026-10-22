//! Keep controller-owned IPC evidence in one private state object.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::config::{HostConfig, RepositoryConnection};
use crate::error::HostError;
use crate::launch::ReadinessSample;

mod readiness;
pub(crate) use readiness::ControllerReadinessProof;
mod session_polls;
pub(crate) use session_polls::SessionPollReceipt;

const DOCTOR_REPORT_MAX_AGE: Duration = Duration::from_secs(30);

/// Live config bindings read by the private request listener.
#[derive(Debug, Clone)]
pub(crate) struct ControlState {
    config: Arc<RwLock<Option<HostConfig>>>,
    pub(super) bindings: Arc<RwLock<Vec<RepositoryConnection>>>,
    session_polls: Arc<RwLock<BTreeMap<String, session_polls::SessionPollReceipt>>>,
    pending_polls: Arc<RwLock<BTreeMap<String, session_polls::PendingSessionPoll>>>,
    readiness: Arc<RwLock<Option<readiness::ControllerReadinessProof>>>,
    readiness_sample: Arc<RwLock<Option<ReadinessSample>>>,
    admission_ready: Arc<AtomicBool>,
    doctor_report: Arc<RwLock<Option<(String, serde_json::Value, Instant)>>>,
}

impl ControlState {
    #[cfg(test)]
    pub(super) fn empty() -> Self {
        Self {
            config: Arc::new(RwLock::new(None)),
            bindings: Arc::new(RwLock::new(Vec::new())),
            session_polls: Arc::new(RwLock::new(BTreeMap::new())),
            pending_polls: Arc::new(RwLock::new(BTreeMap::new())),
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
            readiness: Arc::new(RwLock::new(None)),
            readiness_sample: Arc::new(RwLock::new(None)),
            admission_ready: Arc::new(AtomicBool::new(false)),
            doctor_report: Arc::new(RwLock::new(None)),
        }
    }

    pub(crate) fn replace_config(&self, config: &HostConfig) -> Result<(), HostError> {
        *self.config.write().map_err(|_| HostError::Journal)? = Some(config.clone());
        let mut bindings = self.bindings.write().map_err(|_| HostError::Journal)?;
        *bindings = config.connections().to_vec();
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
        self.admission_ready.store(ready, Ordering::Release);
        if !ready {
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
