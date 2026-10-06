//! In-memory Docker engine for stage unit tests.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::action_archive_seed::ActionArchiveLease;
use crate::error::HostError;
use crate::journal::LaunchIdentity;
use crate::stage::{ContainerRecord, DindProbe, PairEngine, WorkerVolume};
use crate::worker::{CreateProjection, container_labels, container_name, identity_labels_match};

pub(super) struct Fake {
    events: Mutex<Vec<&'static str>>,
    ids: Mutex<HashMap<String, ContainerRecord>>,
    names: Mutex<HashMap<String, String>>,
    pub(super) probes: Mutex<VecDeque<Result<DindProbe, HostError>>>,
    next_id: AtomicU64,
    start_count: AtomicU64,
    pub(super) fail_start_at: Mutex<Option<u64>>,
    pub(super) lose_create_response_at: Mutex<Option<u64>>,
    create_count: AtomicU64,
    pub(super) fail_jit: Mutex<bool>,
    pub(super) fail_remove: Mutex<bool>,
    pub(super) fail_volumes: Mutex<bool>,
    pub(super) inspect_error: Mutex<Option<HostError>>,
}

impl Fake {
    pub(super) fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            ids: Mutex::new(HashMap::new()),
            names: Mutex::new(HashMap::new()),
            probes: Mutex::new(VecDeque::new()),
            next_id: AtomicU64::new(1),
            start_count: AtomicU64::new(0),
            fail_start_at: Mutex::new(None),
            lose_create_response_at: Mutex::new(None),
            create_count: AtomicU64::new(0),
            fail_jit: Mutex::new(false),
            fail_remove: Mutex::new(false),
            fail_volumes: Mutex::new(false),
            inspect_error: Mutex::new(None),
        }
    }

    pub(super) fn events(&self) -> Result<Vec<&'static str>, HostError> {
        self.events
            .lock()
            .map(|events| events.clone())
            .map_err(|_| HostError::Docker)
    }

    pub(super) fn stop(&self, id: &str) -> Result<(), HostError> {
        let mut rows = self.ids.lock().map_err(|_| HostError::Docker)?;
        let record = rows.get_mut(id).ok_or(HostError::Docker)?;
        record.running = Some(false);
        Ok(())
    }

    pub(super) fn containers(&self) -> Result<usize, HostError> {
        self.ids
            .lock()
            .map(|rows| rows.len())
            .map_err(|_| HostError::Docker)
    }

    pub(super) fn removed_volume_count(&self) -> Result<usize, HostError> {
        self.events
            .lock()
            .map(|events| {
                events
                    .iter()
                    .filter(|event| event.starts_with("remove-") && event.ends_with("-volume"))
                    .count()
            })
            .map_err(|_| HostError::Docker)
    }

    fn push(&self, event: &'static str) -> Result<(), HostError> {
        self.events
            .lock()
            .map_err(|_| HostError::Docker)?
            .push(event);
        Ok(())
    }
}

#[expect(
    clippy::unused_async_trait_impl,
    reason = "fake engine matches the async trait and does not await"
)]
impl PairEngine for Fake {
    async fn prepare_volumes(&self, _identity: &LaunchIdentity) -> Result<(), HostError> {
        self.push("volumes")
    }

    async fn create(&self, spec: &CreateProjection) -> Result<String, HostError> {
        let call = self.create_count.fetch_add(1, Ordering::Relaxed) + 1;
        self.push("create")?;
        let id = format!("{:064x}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let mut labels = spec
            .labels
            .iter()
            .filter_map(|label| label.split_once('='))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect::<HashMap<String, String>>();
        labels.insert(
            "org.opencontainers.image.version".to_owned(),
            "26.04".to_owned(),
        );
        self.ids.lock().map_err(|_| HostError::Docker)?.insert(
            id.clone(),
            ContainerRecord {
                id: id.clone(),
                labels,
                running: Some(false),
            },
        );
        let name = spec.name.as_deref().ok_or(HostError::Ownership)?;
        self.names
            .lock()
            .map_err(|_| HostError::Docker)?
            .insert(name.to_owned(), id.clone());
        if *self
            .lose_create_response_at
            .lock()
            .map_err(|_| HostError::Docker)?
            == Some(call)
        {
            return Err(HostError::ContainerCreateUncertain);
        }
        Ok(id)
    }

    async fn start(&self, id: &str) -> Result<(), HostError> {
        let call = self.start_count.fetch_add(1, Ordering::Relaxed) + 1;
        if *self.fail_start_at.lock().map_err(|_| HostError::Docker)? == Some(call) {
            return Err(HostError::ContainerStartUncertain);
        }
        self.push("start")?;
        self.ids
            .lock()
            .map_err(|_| HostError::Docker)?
            .get_mut(id)
            .ok_or(HostError::Docker)?
            .running = Some(true);
        Ok(())
    }

    async fn probe_dind(&self, _id: &str) -> Result<DindProbe, HostError> {
        self.push("probe")?;
        self.probes
            .lock()
            .map_err(|_| HostError::Docker)?
            .pop_front()
            .unwrap_or(Ok(DindProbe::Ready))
    }

    async fn list_launch(
        &self,
        identity: &LaunchIdentity,
    ) -> Result<Vec<ContainerRecord>, HostError> {
        self.push("list-launch")?;
        let expected = [
            ("velnor.product", "velnor"),
            ("velnor.instance", identity.instance_id()),
            ("velnor.launch", identity.launch_id()),
            ("velnor.engine", identity.engine_id()),
        ];
        self.ids.lock().map_err(|_| HostError::Docker).map(|rows| {
            rows.values()
                .filter(|record| {
                    expected.iter().all(|(key, value)| {
                        record.labels.get(*key).map(String::as_str) == Some(*value)
                    })
                })
                .cloned()
                .collect()
        })
    }

    async fn verify_container(
        &self,
        identity: &LaunchIdentity,
        role: &str,
        id: &str,
        dind_id: Option<&str>,
        archive_lease: Option<&ActionArchiveLease>,
        require_running: bool,
    ) -> Result<ContainerRecord, HostError> {
        self.push(if role == "dind" {
            "verify-dind"
        } else {
            "verify-runner"
        })?;
        if role == "runner"
            && archive_lease.is_some_and(|lease| lease.launch_id() != identity.launch_id())
        {
            return Err(HostError::Ownership);
        }
        let record = self
            .ids
            .lock()
            .map_err(|_| HostError::Docker)?
            .get(id)
            .cloned()
            .ok_or(HostError::Ownership)?;
        let labels = container_labels(identity, role)
            .iter()
            .filter_map(|label| label.split_once('='))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect::<HashMap<_, _>>();
        let named = self
            .names
            .lock()
            .map_err(|_| HostError::Docker)?
            .get(&container_name(identity, role))
            .cloned();
        if named.as_deref() != Some(id)
            || !identity_labels_match(&labels, &record.labels)
            || (require_running && record.running != Some(true))
        {
            return Err(HostError::Ownership);
        }
        if role == "runner" {
            let dind_id = dind_id.ok_or(HostError::Ownership)?;
            if !self
                .ids
                .lock()
                .map_err(|_| HostError::Docker)?
                .contains_key(dind_id)
            {
                return Err(HostError::Ownership);
            }
        } else if role != "dind" || dind_id.is_some() || archive_lease.is_some() {
            return Err(HostError::Ownership);
        }
        Ok(record)
    }

    async fn write_jit(&self, _id: &str, _jit: &[u8]) -> Result<(), HostError> {
        if *self.fail_jit.lock().map_err(|_| HostError::Docker)? {
            return Err(HostError::JitDeliveryUncertain);
        }
        self.push("jit")
    }

    async fn remove(&self, id: &str) -> Result<(), HostError> {
        if *self.fail_remove.lock().map_err(|_| HostError::Docker)? {
            return Err(HostError::Docker);
        }
        self.push("remove")?;
        self.ids.lock().map_err(|_| HostError::Docker)?.remove(id);
        self.names
            .lock()
            .map_err(|_| HostError::Docker)?
            .retain(|_, found| found != id);
        Ok(())
    }

    async fn inspect_container(
        &self,
        id_or_name: &str,
    ) -> Result<Option<ContainerRecord>, HostError> {
        if let Some(error) = *self.inspect_error.lock().map_err(|_| HostError::Docker)? {
            return Err(error);
        }
        let id = self
            .names
            .lock()
            .map_err(|_| HostError::Docker)?
            .get(id_or_name)
            .cloned()
            .unwrap_or_else(|| id_or_name.to_owned());
        self.ids
            .lock()
            .map_err(|_| HostError::Docker)
            .map(|rows| rows.get(&id).cloned())
    }

    async fn remove_volume(
        &self,
        _identity: &LaunchIdentity,
        volume: WorkerVolume,
    ) -> Result<(), HostError> {
        let event = match volume {
            WorkerVolume::Socket => "remove-socket-volume",
            WorkerVolume::Workspace => "remove-workspace-volume",
            WorkerVolume::DockerData => "remove-docker-volume",
        };
        self.push(event)?;
        if *self.fail_volumes.lock().map_err(|_| HostError::Docker)? {
            return Err(HostError::DockerTimeout);
        }
        Ok(())
    }

    async fn verify_volume(
        &self,
        _identity: &LaunchIdentity,
        volume: WorkerVolume,
    ) -> Result<(), HostError> {
        let event = match volume {
            WorkerVolume::Socket => "verify-socket-volume",
            WorkerVolume::Workspace => "verify-workspace-volume",
            WorkerVolume::DockerData => "verify-docker-volume",
        };
        self.push(event)
    }

    async fn verify_engine(&self, _identity: &LaunchIdentity) -> Result<(), HostError> {
        self.push("engine")
    }
}
