//! Keep one journal, engine, host budget, and live config view.

use std::path::Path;
use std::time::Duration;

use crate::config::{HostConfig, RepositoryConnection};
use crate::controller_ipc::{ControlServer, ControlState};
use crate::daemon_lock::{EngineLineageGuard, canonical_journal_path};
use crate::docker_client::{connect_unix, docker_deadline};
use crate::error::HostError;
use crate::journal::{CapacityLimits, Journal};
use crate::launch_blocking::ListenFault;

const CONFIG_RELOAD_PAUSE: Duration = Duration::from_secs(2);

/// Run the shared controller without acquiring `daemon.lock`.
///
/// The caller keeps the daemon lock for this function's full run.
///
/// # Errors
///
/// Returns [`ListenFault`] if the selected engine, journal, or local controller fails to start.
pub fn run_host_blocking(config: &HostConfig, state: &Path) -> Result<(), ListenFault> {
    let _capacity = crate::launch::install_job_capacity(config.max_jobs());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| HostError::Journal)?;
    runtime.block_on(run_controller(config.clone(), state))
}

async fn run_controller(mut current: HostConfig, state: &Path) -> Result<(), ListenFault> {
    let docker = connect_unix(&current.docker.endpoint)?;
    let engine_id = docker_deadline(docker.info())
        .await?
        .map_err(|_| HostError::Docker)?
        .id
        .filter(|id| !id.trim().is_empty())
        .ok_or(HostError::Docker)?;
    let journal_path = canonical_journal_path(&state.join("launch.db"))?;
    let lineage = EngineLineageGuard::acquire(&engine_id)?;
    let journal = Journal::open(&journal_path).await?;
    journal.bind_engine(&engine_id).await?;
    let instance_id = journal.instance_id().await?;
    let revision = journal.revision().await?;
    lineage.verify_lineage(&journal_path, &instance_id, revision)?;
    journal.attach_lineage_guard(lineage)?;

    let legacy_reconcile = crate::launch::reconcile_gate(&journal, &docker).await?;
    eprintln!(
        "legacy_reconcile={}",
        crate::launch::gate_line(&legacy_reconcile)
    );
    configure_shared_capacity(&journal, &current).await?;
    bind_active_connections(&journal, &current, &engine_id).await?;
    let (_server, control_state) = ControlServer::start(state, journal.clone(), &current)?;
    let mut pending_retirements = Vec::new();
    discover_retirements(state, &current, &mut pending_retirements);

    // STE: Work polling stays closed until the journal exposes a durable
    // host-wide header-allocation and acknowledgment ledger.
    loop {
        tokio::time::sleep(CONFIG_RELOAD_PAUSE).await;
        let config_path = state.join("host.toml");
        let next = match super::config_watch::read_host_config(&config_path) {
            Ok(next) => next,
            Err(error) => {
                eprintln!("host controller config reload held: {error}");
                None
            }
        };
        let mut effects = JournalEffects {
            state,
            journal: &journal,
            control_state: &control_state,
            engine_id: &engine_id,
        };
        if let Some(next) = next
            && next != current
        {
            match apply_config_transition(&current, &next, &mut effects).await {
                Ok(removed) => {
                    current = next;
                    pending_retirements.extend(removed);
                }
                Err(error) => {
                    eprintln!("host controller config transition held: {error}");
                }
            }
        }
        discover_retirements(state, &current, &mut pending_retirements);
        if let Err(error) = retire_pending(&mut pending_retirements, &mut effects).await {
            eprintln!("host controller connection retirement held: {error}");
        }
    }
}

fn discover_retirements(
    state: &Path,
    config: &HostConfig,
    pending: &mut Vec<RepositoryConnection>,
) {
    match super::retirement::removed_bindings(state, config) {
        Ok(bindings) => {
            for binding in bindings {
                if !pending.iter().any(|existing| existing == &binding) {
                    pending.push(binding);
                }
            }
        }
        Err(error) => eprintln!("host controller retirement scan held: {error}"),
    }
}

async fn configure_shared_capacity(
    journal: &Journal,
    config: &HostConfig,
) -> Result<(), HostError> {
    let budget = config.resource_budget()?;
    let pair = budget.pair();
    let jobs = config.max_jobs();
    let cpu_millicores = pair
        .cpu_millicores
        .checked_mul(u64::from(jobs))
        .ok_or(HostError::Config)?;
    let memory_bytes = pair
        .memory_bytes
        .checked_mul(u64::from(jobs))
        .ok_or(HostError::Config)?;
    journal
        .configure_capacity(CapacityLimits {
            jobs,
            cpu_millicores,
            memory_bytes,
        })
        .await
}

async fn bind_active_connections(
    journal: &Journal,
    config: &HostConfig,
    engine_id: &str,
) -> Result<(), HostError> {
    for binding in config.connections() {
        bind_connection(journal, binding, config, engine_id).await?;
    }
    Ok(())
}

async fn bind_connection(
    journal: &Journal,
    binding: &RepositoryConnection,
    config: &HostConfig,
    engine_id: &str,
) -> Result<(), HostError> {
    let scoped = journal.connection_scope(binding).await?;
    scoped
        .bind_engine_context(engine_id, &config.docker.endpoint, &config.docker.platform)
        .await
}

pub(super) trait TransitionEffects {
    async fn configure_shared_budget(&mut self, config: &HostConfig) -> Result<(), HostError>;
    async fn bind_connection(
        &mut self,
        binding: &RepositoryConnection,
        config: &HostConfig,
    ) -> Result<(), HostError>;
    fn publish_config(&mut self, config: &HostConfig) -> Result<(), HostError>;
    async fn retire_removed(&mut self, binding: &RepositoryConnection) -> Result<bool, HostError>;
}

/// Apply one config without changing a binding or making a budget per connection.
///
/// Return removed bindings after the new IPC view is active.
pub(super) async fn apply_config_transition<E: TransitionEffects>(
    previous: &HostConfig,
    next: &HostConfig,
    effects: &mut E,
) -> Result<Vec<RepositoryConnection>, HostError> {
    validate_transition(previous, next)?;
    effects.configure_shared_budget(next).await?;
    for binding in next.connections() {
        effects.bind_connection(binding, next).await?;
    }
    effects.publish_config(next)?;

    Ok(previous
        .connections()
        .iter()
        .filter(|binding| next.connection(&binding.connection_id).is_none())
        .cloned()
        .collect())
}

pub(super) async fn retire_pending<E: TransitionEffects>(
    pending: &mut Vec<RepositoryConnection>,
    effects: &mut E,
) -> Result<(), HostError> {
    let mut remaining = Vec::new();
    for binding in pending.iter() {
        if !effects.retire_removed(binding).await? {
            remaining.push(binding.clone());
        }
    }
    *pending = remaining;
    Ok(())
}

fn validate_transition(previous: &HostConfig, next: &HostConfig) -> Result<(), HostError> {
    if previous.docker != next.docker
        || previous.connections().iter().any(|current| {
            next.connection(&current.connection_id)
                .is_some_and(|candidate| candidate != current)
        })
    {
        return Err(HostError::Config);
    }
    Ok(())
}

struct JournalEffects<'a> {
    state: &'a Path,
    journal: &'a Journal,
    control_state: &'a ControlState,
    engine_id: &'a str,
}

impl TransitionEffects for JournalEffects<'_> {
    async fn configure_shared_budget(&mut self, config: &HostConfig) -> Result<(), HostError> {
        configure_shared_capacity(self.journal, config).await
    }

    async fn bind_connection(
        &mut self,
        binding: &RepositoryConnection,
        config: &HostConfig,
    ) -> Result<(), HostError> {
        let scoped = self.journal.connection_scope(binding).await?;
        scoped
            .bind_engine_context(
                self.engine_id,
                &config.docker.endpoint,
                &config.docker.platform,
            )
            .await
    }

    fn publish_config(&mut self, config: &HostConfig) -> Result<(), HostError> {
        self.control_state.replace_config(config)
    }

    async fn retire_removed(&mut self, binding: &RepositoryConnection) -> Result<bool, HostError> {
        super::retirement::retire_if_removed(self.state, self.journal, self.control_state, binding)
            .await
    }
}
