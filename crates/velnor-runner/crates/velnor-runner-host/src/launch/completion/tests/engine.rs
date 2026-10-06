use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

use crate::error::HostError;
use crate::launch_identity::LaunchIdentity;
use crate::stage::{ContainerRecord, DindProbe, PairEngine};
use crate::worker::{CreateProjection, container_labels, container_name};

#[derive(Clone)]
pub(super) struct CompletionEngine {
    identity: Option<LaunchIdentity>,
    records: Arc<Mutex<HashMap<String, ContainerRecord>>>,
    names: Arc<Mutex<HashMap<String, String>>>,
    pub(super) removed_volumes: Arc<AtomicBool>,
    pub(super) fail_volumes: Arc<AtomicBool>,
    verify_delay: Arc<AtomicBool>,
    verify_sender: Option<mpsc::Sender<()>>,
    verify_barrier_used: Arc<AtomicBool>,
    volumes_sender: Option<mpsc::Sender<()>>,
}

impl CompletionEngine {
    pub(super) fn with_stopped_pair(
        identity: &LaunchIdentity,
        runner_id: &str,
        dind_id: &str,
    ) -> Result<Self, String> {
        let mut records = HashMap::new();
        let mut names = HashMap::new();
        for (role, id, running) in [("runner", runner_id, false), ("dind", dind_id, true)] {
            records.insert(
                id.to_owned(),
                ContainerRecord {
                    id: id.to_owned(),
                    labels: labels(identity, role),
                    running: Some(running),
                },
            );
            names.insert(container_name(identity, role), id.to_owned());
        }
        Ok(Self::new(Some(identity.clone()), records, names))
    }

    pub(super) fn empty() -> Self {
        Self::new(None, HashMap::new(), HashMap::new())
    }

    pub(super) fn with_progress_senders(
        mut self,
        verify: mpsc::Sender<()>,
        volumes: mpsc::Sender<()>,
    ) -> Self {
        self.verify_sender = Some(verify);
        self.volumes_sender = Some(volumes);
        self
    }

    pub(super) fn with_verify_timer(self) -> Self {
        self.verify_delay.store(true, Ordering::Release);
        self
    }

    fn new(
        identity: Option<LaunchIdentity>,
        records: HashMap<String, ContainerRecord>,
        names: HashMap<String, String>,
    ) -> Self {
        Self {
            identity,
            records: Arc::new(Mutex::new(records)),
            names: Arc::new(Mutex::new(names)),
            removed_volumes: Arc::new(AtomicBool::new(false)),
            fail_volumes: Arc::new(AtomicBool::new(false)),
            verify_delay: Arc::new(AtomicBool::new(false)),
            verify_sender: None,
            verify_barrier_used: Arc::new(AtomicBool::new(false)),
            volumes_sender: None,
        }
    }
}

#[expect(
    clippy::unused_async_trait_impl,
    reason = "the test engine has no external waits"
)]
impl PairEngine for CompletionEngine {
    async fn prepare_volumes(&self, _identity: &LaunchIdentity) -> Result<(), HostError> {
        Ok(())
    }

    async fn create(&self, _spec: &CreateProjection) -> Result<String, HostError> {
        Err(HostError::Docker)
    }

    async fn start(&self, _id: &str) -> Result<(), HostError> {
        Err(HostError::Docker)
    }

    async fn probe_dind(&self, _id: &str) -> Result<DindProbe, HostError> {
        Err(HostError::Docker)
    }

    async fn list_launch(
        &self,
        identity: &LaunchIdentity,
    ) -> Result<Vec<ContainerRecord>, HostError> {
        if self
            .identity
            .as_ref()
            .is_some_and(|expected| expected != identity)
        {
            return Err(HostError::Ownership);
        }
        self.records
            .lock()
            .map(|rows| rows.values().cloned().collect())
            .map_err(|_| HostError::Docker)
    }

    async fn verify_container(
        &self,
        identity: &LaunchIdentity,
        role: &str,
        id: &str,
        dind_id: Option<&str>,
        require_running: bool,
    ) -> Result<ContainerRecord, HostError> {
        if self
            .identity
            .as_ref()
            .is_some_and(|expected| expected != identity)
            || (role == "runner" && dind_id.is_none())
        {
            return Err(HostError::Ownership);
        }
        let record = self
            .inspect_container(id)
            .await?
            .ok_or(HostError::Ownership)?;
        if record.labels != labels(identity, role)
            || (require_running && record.running != Some(true))
        {
            return Err(HostError::Ownership);
        }
        Ok(record)
    }

    async fn write_jit(&self, _id: &str, _jit: &[u8]) -> Result<(), HostError> {
        Err(HostError::Docker)
    }

    async fn remove(&self, id: &str) -> Result<(), HostError> {
        self.records
            .lock()
            .map_err(|_| HostError::Docker)?
            .remove(id)
            .ok_or(HostError::Ownership)?;
        self.names
            .lock()
            .map_err(|_| HostError::Docker)?
            .retain(|_, value| value != id);
        Ok(())
    }

    async fn inspect_container(
        &self,
        id_or_name: &str,
    ) -> Result<Option<ContainerRecord>, HostError> {
        let id = self
            .names
            .lock()
            .map_err(|_| HostError::Docker)?
            .get(id_or_name)
            .cloned()
            .unwrap_or_else(|| id_or_name.to_owned());
        Ok(self
            .records
            .lock()
            .map_err(|_| HostError::Docker)?
            .get(&id)
            .cloned())
    }

    async fn remove_volumes(&self, _identity: &LaunchIdentity) -> Result<(), HostError> {
        if self.fail_volumes.load(Ordering::Acquire) {
            return Err(HostError::Cleanup);
        }
        self.removed_volumes.store(true, Ordering::Release);
        if let Some(sender) = &self.volumes_sender {
            sender.send(()).map_err(|_| HostError::Docker)?;
        }
        Ok(())
    }

    async fn verify_engine(&self, identity: &LaunchIdentity) -> Result<(), HostError> {
        if !self.verify_barrier_used.swap(true, Ordering::AcqRel) {
            if let Some(sender) = &self.verify_sender {
                sender.send(()).map_err(|_| HostError::Docker)?;
            }
        }
        if self.verify_delay.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if self
            .identity
            .as_ref()
            .is_none_or(|expected| expected == identity)
        {
            Ok(())
        } else {
            Err(HostError::Ownership)
        }
    }
}

fn labels(identity: &LaunchIdentity, role: &str) -> HashMap<String, String> {
    container_labels(identity, role)
        .iter()
        .filter_map(|label| label.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}
