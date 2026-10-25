//! Own the controller loop and configuration refresh lifecycle.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use crate::{
    config::HostConfig,
    controller_ipc::ControlServer,
    daemon_lock::{DaemonLock, EngineLineageGuard, canonical_journal_path},
    docker_client::{connect_unix, docker_deadline},
    error::HostError,
    journal::{EngineContext, Journal},
    launch::{ReleaseReadSecret, SelectedEngineContext, startup_storage_profile},
    launch_blocking::ListenFault,
};

use super::supervision::LocalWorkerHealthSupervisor;
use super::{
    connection_poll::{ConnectionPollContext, poll_active_connections},
    setup,
    state::setup_receipt_inputs,
    transitions::{JournalEffects, apply_config, retire_pending},
};
#[path = "controller/doctor.rs"]
mod doctor;
#[path = "controller/readiness.rs"]
mod readiness;
#[path = "controller/startup.rs"]
mod startup;
#[path = "controller/worker_health.rs"]
mod worker_health;

pub(super) use startup::run_controller;

use doctor::DoctorProbeWorker;

const CONFIG_RELOAD_PAUSE: Duration = Duration::from_secs(2);
const STORAGE_PROFILE_RETRY: Duration = Duration::from_secs(30);

pub(super) struct ControllerBase {
    pub(super) state: PathBuf,
    pub(super) docker: bollard::Docker,
    pub(super) selected_engine: SelectedEngineContext,
    pub(super) journal: Journal,
    pub(super) lineage: EngineLineageGuard,
}

pub(super) async fn open_controller_base(
    current: &HostConfig,
    state: &Path,
    daemon_lock: &DaemonLock,
) -> Result<ControllerBase, ListenFault> {
    let state = std::fs::canonicalize(state).map_err(|_| HostError::Path)?;
    if !state.is_dir() || !daemon_lock.matches_path(&state.join("daemon.lock")) {
        return Err(HostError::Lock.into());
    }
    let docker = connect_unix(&current.docker.endpoint)?;
    let info = docker_deadline(docker.info())
        .await?
        .map_err(|_| HostError::Docker)?;
    let engine_id = info
        .id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .ok_or(HostError::Docker)?;
    let durable_engine = EngineContext {
        engine_id: engine_id.clone(),
        endpoint: current.docker.endpoint.clone(),
        platform: current.docker.platform.clone(),
    };
    let selected_engine = SelectedEngineContext::capture(&durable_engine, &info)?;
    let journal_path = canonical_journal_path(&state.join("launch.db"))?;
    let lineage = EngineLineageGuard::acquire(&engine_id)?;
    let journal = Journal::open(&journal_path).await?;
    journal.bind_engine(&engine_id).await?;
    let instance_id = journal.instance_id().await?;
    let revision = journal.revision().await?;
    lineage.verify_lineage(&journal_path, &instance_id, revision)?;
    journal.attach_lineage_guard(lineage.clone())?;
    Ok(ControllerBase {
        state,
        docker,
        selected_engine,
        journal,
        lineage,
    })
}

struct ControllerLoop<'a> {
    current: HostConfig,
    state: PathBuf,
    daemon_lock: &'a DaemonLock,
    docker: bollard::Docker,
    selected_engine: SelectedEngineContext,
    journal: Journal,
    worker_images: crate::launch::VerifiedWorkerImages,
    _control_server: ControlServer,
    control_state: crate::controller_ipc::ControlState,
    connection_sessions: Vec<super::state::PreparedConnection>,
    pending_retirements: Vec<crate::config::RepositoryConnection>,
    local_worker_health: LocalWorkerHealthSupervisor,
    doctor_worker: Option<DoctorProbeWorker>,
    _readiness_worker: readiness::ReadinessWorker,
    doctor_attempted_request_id: Option<i64>,
    release_read_secret: Arc<ReleaseReadSecret>,
    admission_ready: bool,
    rotation_due: bool,
    next_profile_attempt: tokio::time::Instant,
}

impl<'a> ControllerLoop<'a> {
    async fn initialize(
        current: HostConfig,
        state: &Path,
        daemon_lock: &'a DaemonLock,
        release_read_secret: ReleaseReadSecret,
    ) -> Result<Self, ListenFault> {
        let base = open_controller_base(&current, state, daemon_lock).await?;
        Self::finish_initialization(current, daemon_lock, release_read_secret, base).await
    }

    async fn finish_initialization(
        current: HostConfig,
        daemon_lock: &'a DaemonLock,
        release_read_secret: ReleaseReadSecret,
        base: ControllerBase,
    ) -> Result<Self, ListenFault> {
        let release_read_secret = Arc::new(release_read_secret);
        let ControllerBase {
            state,
            docker,
            selected_engine,
            journal,
            lineage,
        } = base;
        let startup = startup::prepare_startup(startup::StartupInputs {
            current: &current,
            daemon_lock,
            state: &state,
            docker: &docker,
            selected_engine: &selected_engine,
            journal: &journal,
            lineage: &lineage,
            release_read_secret: &release_read_secret,
        })
        .await?;
        let worker_images = startup.worker_images;
        let connection_sessions = setup::prepare_active_connections(
            &state,
            &journal,
            &current,
            &selected_engine,
            &worker_images,
            &release_read_secret,
            &docker,
        )
        .await?;
        let (control_server, control_state) = ControlServer::start(
            &state,
            journal.clone(),
            &current,
            selected_engine.engine_id().to_owned(),
            selected_engine.docker_root_digest().to_owned(),
        )?;
        control_state.replace_setup_receipts(setup_receipt_inputs(
            &connection_sessions,
            &current,
            selected_engine.engine_id(),
            selected_engine.docker_root_digest(),
        )?)?;
        control_state.set_admission_ready(startup.admission_ready);
        let readiness_worker = readiness::ReadinessWorker::start(
            state.clone(),
            docker.clone(),
            journal.clone(),
            selected_engine.clone(),
            control_state.clone(),
            Arc::clone(&release_read_secret),
        )?;
        let mut pending_retirements = Vec::new();
        setup::discover_retirements(&state, &current, &mut pending_retirements);
        Ok(Self {
            current,
            state,
            daemon_lock,
            docker,
            selected_engine,
            journal,
            worker_images,
            _control_server: control_server,
            control_state,
            connection_sessions,
            pending_retirements,
            local_worker_health: LocalWorkerHealthSupervisor::default(),
            doctor_worker: None,
            _readiness_worker: readiness_worker,
            doctor_attempted_request_id: None,
            release_read_secret,
            admission_ready: startup.admission_ready,
            rotation_due: false,
            next_profile_attempt: tokio::time::Instant::now() + STORAGE_PROFILE_RETRY,
        })
    }

    async fn run(mut self) -> Result<(), ListenFault> {
        // Positive work uses scoped admission proofs; zero-header polling is for lifecycle messages.
        loop {
            tokio::time::sleep(CONFIG_RELOAD_PAUSE).await;
            self.start_doctor_probe().await;
            self.refresh_storage_profile().await;
            self.refresh_config_and_retire().await?;
            self.supervise_local_workers().await;
            Box::pin(self.poll_connections()).await;
        }
    }

    async fn refresh_config_and_retire(&mut self) -> Result<(), ListenFault> {
        if !self
            .daemon_lock
            .matches_path(&self.state.join("daemon.lock"))
        {
            return Err(HostError::Lock.into());
        }
        let next = match crate::host_controller::config_watch::read_host_config(
            &self.state.join("host.toml"),
        ) {
            Ok(next) => next,
            Err(error) => {
                eprintln!("host controller config reload held: {error}");
                None
            }
        };
        let mut effects = JournalEffects {
            state: &self.state,
            journal: &self.journal,
            control_state: &self.control_state,
            selected_engine: &self.selected_engine,
            worker_images: &self.worker_images,
            release_read_secret: &self.release_read_secret,
            docker: &self.docker,
            connection_sessions: &mut self.connection_sessions,
        };
        if let Some(next) = next
            && next != self.current
        {
            let changed = apply_config(
                &mut self.current,
                &mut self.pending_retirements,
                next,
                &mut effects,
            )
            .await;
            if changed {
                self.admission_ready = false;
                self.control_state.set_admission_ready(false);
                self.next_profile_attempt = tokio::time::Instant::now();
            }
        }
        setup::discover_retirements(&self.state, &self.current, &mut self.pending_retirements);
        if let Err(error) = retire_pending(&mut self.pending_retirements, &mut effects).await {
            eprintln!("host controller connection retirement held: {error}");
        }
        Ok(())
    }

    async fn poll_connections(&mut self) {
        let fairness_enabled = u32::try_from(self.current.connections().len())
            .is_ok_and(|count| count > self.current.max_jobs());
        let outcome = Box::pin(poll_active_connections(
            ConnectionPollContext {
                config: &self.current,
                journal: &self.journal,
                docker: &self.docker,
                selected_engine: &self.selected_engine,
                worker_images: &self.worker_images,
                state: &self.state,
                release_read_secret: &self.release_read_secret,
                admission_ready: self.admission_ready,
                control_state: &self.control_state,
            },
            &self.pending_retirements,
            &mut self.connection_sessions,
            super::connection_poll::fairness::FairnessTurn::new(
                fairness_enabled,
                self.rotation_due,
            ),
        ))
        .await;
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                eprintln!("host controller zero-header polling held: {error}");
                return;
            }
        };
        if fairness_enabled && outcome.positive_poll_settled {
            self.rotation_due = true;
        }
        if self.rotation_due && outcome.rotation_complete {
            match setup::prepare_active_connections(
                &self.state,
                &self.journal,
                &self.current,
                &self.selected_engine,
                &self.worker_images,
                &self.release_read_secret,
                &self.docker,
            )
            .await
            {
                Ok(prepared) => {
                    let setup_receipts = match setup_receipt_inputs(
                        &prepared,
                        &self.current,
                        self.selected_engine.engine_id(),
                        self.selected_engine.docker_root_digest(),
                    ) {
                        Ok(receipts) => receipts,
                        Err(error) => {
                            eprintln!(
                                "The controller could not prepare connection setup receipts. Error: {error}."
                            );
                            return;
                        }
                    };
                    if let Err(error) = self.control_state.replace_setup_receipts(setup_receipts) {
                        eprintln!(
                            "The controller could not replace connection setup receipts. Error: {error}."
                        );
                        return;
                    }
                    self.connection_sessions = prepared;
                    self.rotation_due = false;
                }
                Err(error) => {
                    eprintln!(
                        "The host controller cannot assign capacity to the next connection: {error}"
                    );
                }
            }
        }
    }

    async fn refresh_storage_profile(&mut self) {
        if self.admission_ready || tokio::time::Instant::now() < self.next_profile_attempt {
            return;
        }
        self.next_profile_attempt = tokio::time::Instant::now() + STORAGE_PROFILE_RETRY;
        let profile = startup_storage_profile(
            &self.docker,
            &self.journal,
            match self.current.resource_budget() {
                Ok(budget) => budget,
                Err(error) => {
                    eprintln!("host controller storage profile refresh held: {error}");
                    return;
                }
            },
            self.current.max_jobs(),
            &self.release_read_secret,
            &self.worker_images,
        )
        .await;
        let Some(profile) = profile else {
            eprintln!("host controller storage profile remains unavailable");
            return;
        };
        match self.journal.configure_storage_profile(profile).await {
            Ok(()) => {
                let receipts = match setup_receipt_inputs(
                    &self.connection_sessions,
                    &self.current,
                    self.selected_engine.engine_id(),
                    self.selected_engine.docker_root_digest(),
                ) {
                    Ok(receipts) => receipts,
                    Err(error) => {
                        eprintln!("host controller setup receipt refresh held: {error}");
                        return;
                    }
                };
                if let Err(error) = self.control_state.replace_setup_receipts(receipts) {
                    eprintln!("host controller setup receipt refresh held: {error}");
                    return;
                }
                self.admission_ready = true;
                self.control_state.set_admission_ready(true);
            }
            Err(error) => {
                self.control_state.set_admission_ready(false);
                eprintln!("The controller could not store the storage profile. Error: {error}.");
            }
        }
    }
}
