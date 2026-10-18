//! Keep one journal, engine, host budget, and live config view.

use std::path::Path;
use std::str;
use std::time::Duration;

use bollard::Docker;
use velnor_runner_github::QueueSession;

use crate::config::{HostConfig, RepositoryConnection};
use crate::controller_ipc::{ControlServer, ControlState};
use crate::daemon_lock::{DaemonLock, EngineLineageGuard, canonical_journal_path};
use crate::docker_client::{connect_unix, docker_deadline};
use crate::error::HostError;
use crate::journal::{
    CapacityLimits, ConnectionJournal, EngineContext, Journal, OperationalCapacityLimits,
};
use crate::launch::{
    CompletionWorker, ReleaseReadSecret, SelectedEngineContext, SessionDeleteOutcome,
    SessionRefreshStartupAuthority, delete_removed_connection_session, open_connection_session,
    poll_scoped_connection_once,
};
use crate::launch_blocking::ListenFault;
use crate::listen::Link;
use crate::scale_set::{ConnectionSetup, ensure_connection_scale_set};

const CONFIG_RELOAD_PAUSE: Duration = Duration::from_secs(2);

/// Run the shared controller without acquiring `daemon.lock`.
///
/// The caller keeps the daemon lock for this function's full run.
///
/// # Errors
///
/// Returns [`ListenFault`] if the selected engine, journal, or local controller fails to start.
pub fn run_host_blocking(
    config: &HostConfig,
    state: &Path,
    daemon_lock: &DaemonLock,
) -> Result<(), ListenFault> {
    let state_root = std::fs::canonicalize(state).map_err(|_| HostError::Path)?;
    if !state_root.is_dir() {
        return Err(HostError::Path.into());
    }
    if !daemon_lock.matches_path(&state_root.join("daemon.lock")) {
        return Err(HostError::Lock.into());
    }
    let release_read_secret = ReleaseReadSecret::load(config)?;
    let _capacity = crate::launch::install_job_capacity(config.max_jobs());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| HostError::Journal)?;
    runtime.block_on(run_controller(
        config.clone(),
        state,
        daemon_lock,
        release_read_secret,
    ))
}

async fn run_controller(
    mut current: HostConfig,
    state: &Path,
    daemon_lock: &DaemonLock,
    mut _release_read_secret: ReleaseReadSecret,
) -> Result<(), ListenFault> {
    let state_root = std::fs::canonicalize(state).map_err(|_| HostError::Path)?;
    if !state_root.is_dir() {
        return Err(HostError::Path.into());
    }
    let state = state_root.as_path();
    let daemon_lock_path = state.join("daemon.lock");
    if !daemon_lock.matches_path(&daemon_lock_path) {
        return Err(HostError::Lock.into());
    }

    let docker = connect_unix(&current.docker.endpoint)?;
    let selected_info = docker_deadline(docker.info())
        .await?
        .map_err(|_| HostError::Docker)?;
    let engine_id = selected_info
        .id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .ok_or(HostError::Docker)?;
    let durable_engine = EngineContext {
        engine_id: engine_id.clone(),
        endpoint: current.docker.endpoint.clone(),
        platform: current.docker.platform.clone(),
    };
    let selected_engine = SelectedEngineContext::capture(&durable_engine, &selected_info)?;
    let journal_path = canonical_journal_path(&state.join("launch.db"))?;
    let lineage = EngineLineageGuard::acquire(&engine_id)?;
    let journal = Journal::open(&journal_path).await?;
    journal.bind_engine(&engine_id).await?;
    let instance_id = journal.instance_id().await?;
    let revision = journal.revision().await?;
    lineage.verify_lineage(&journal_path, &instance_id, revision)?;
    journal.attach_lineage_guard(lineage.clone())?;

    let legacy_reconcile = crate::launch::reconcile_gate(&journal, &docker).await?;
    eprintln!(
        "legacy_reconcile={}",
        crate::launch::gate_line(&legacy_reconcile)
    );
    configure_shared_capacity(&journal, &current).await?;
    journal.bootstrap_header_authority().await?;
    journal.validate_header_authority().await?;
    let fresh_info = docker_deadline(docker.info())
        .await?
        .map_err(|_| HostError::Docker)?;
    let startup_authority = SessionRefreshStartupAuthority::after_validated_startup(
        daemon_lock,
        state,
        &lineage,
        &journal,
        &selected_engine,
        &fresh_info,
    )
    .await?;
    journal
        .recover_interrupted_session_refreshes(startup_authority)
        .await?;
    let mut connection_sessions =
        prepare_active_connections(&journal, &current, &selected_engine, &docker).await?;
    let (_server, control_state) = ControlServer::start(state, journal.clone(), &current)?;
    let mut pending_retirements = Vec::new();
    discover_retirements(state, &current, &mut pending_retirements);

    // Zero-header polling handles lifecycle messages only. Positive work
    // admission remains closed until every durable admission proof is wired.
    loop {
        tokio::time::sleep(CONFIG_RELOAD_PAUSE).await;
        if !daemon_lock.matches_path(&daemon_lock_path) {
            return Err(HostError::Lock.into());
        }
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
            selected_engine: &selected_engine,
            docker: &docker,
            connection_sessions: &mut connection_sessions,
        };
        if let Some(next) = next
            && next != current
        {
            match ReleaseReadSecret::load(&next) {
                Ok(next_secret) => {
                    match apply_config_transition(&current, &next, &mut effects).await {
                        Ok(removed) => {
                            current = next;
                            _release_read_secret = next_secret;
                            pending_retirements.extend(removed);
                        }
                        Err(error) => {
                            eprintln!("host controller config transition held: {error}");
                        }
                    }
                }
                Err(error) => eprintln!("host controller release-reader reload held: {error}"),
            }
        }
        discover_retirements(state, &current, &mut pending_retirements);
        if let Err(error) = retire_pending(&mut pending_retirements, &mut effects).await {
            eprintln!("host controller connection retirement held: {error}");
        }
        drop(effects);
        if let Err(error) =
            poll_active_connections(&current, &pending_retirements, &mut connection_sessions).await
        {
            eprintln!("host controller zero-header polling held: {error}");
        }
    }
}

async fn poll_active_connections(
    config: &HostConfig,
    pending_retirements: &[RepositoryConnection],
    connections: &mut Vec<PreparedConnection>,
) -> Result<(), HostError> {
    let mut index = 0;
    while index < connections.len() {
        let prepared = &mut connections[index];
        let is_removed = config.connection(&prepared.binding.connection_id).is_none();
        if is_removed
            && !pending_retirements
                .iter()
                .any(|binding| prepared.is_for(binding))
        {
            index += 1;
            continue;
        }
        let Some(credentials) = prepared.credentials.as_mut() else {
            index += 1;
            continue;
        };
        let allocation = match prepared
            .scope
            .session_header_allocation(prepared.session_intent)
            .await
        {
            Ok(allocation) => allocation,
            Err(error) => {
                eprintln!(
                    "host controller polling held connection={} reason={error}",
                    prepared.binding.connection_id
                );
                prepared.credentials.take();
                index += 1;
                continue;
            }
        };
        // A removed binding must pass the read-only drain check before its
        // final zero poll. The delete claim repeats this check atomically.
        let drained_before_poll = if is_removed {
            match prepared
                .scope
                .session_work_drained(prepared.session_intent)
                .await
            {
                Ok(drained) => drained,
                Err(error) => {
                    eprintln!(
                        "host controller drain check held connection={} reason={error}",
                        prepared.binding.connection_id
                    );
                    prepared.credentials.take();
                    index += 1;
                    continue;
                }
            }
        } else {
            false
        };
        let outcome = poll_scoped_connection_once(
            &mut credentials.link,
            prepared.set_id,
            &credentials.session,
            &prepared.scope,
            prepared.session_intent,
            &allocation,
        )
        .await;
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                eprintln!(
                    "host controller poll held connection={} reason={error}",
                    prepared.binding.connection_id
                );
                prepared.credentials.take();
                index += 1;
                continue;
            }
        };
        if outcome.wake_cleanup
            && let Some(completion) = prepared.completion.as_ref()
        {
            completion.notify();
        }

        let should_delete = is_removed && drained_before_poll && outcome.session_delete_candidate;
        let mut remove_prepared = false;
        if should_delete {
            match delete_removed_connection_session(
                &mut credentials.link,
                &credentials.session,
                &prepared.scope,
                prepared.session_intent,
                &allocation,
            )
            .await
            {
                SessionDeleteOutcome::NotReady => {}
                SessionDeleteOutcome::Deleted => {
                    prepared.credentials.take();
                    if let Some(completion) = prepared.completion.take()
                        && let Err(error) = completion.shutdown().await
                    {
                        eprintln!(
                            "host controller completion shutdown held connection={} reason={error}",
                            prepared.binding.connection_id
                        );
                    }
                    remove_prepared = true;
                }
                SessionDeleteOutcome::Held => {
                    // The DELETE may have completed without a durable 204
                    // acknowledgement. Keep its allocation and stop all use
                    // of this session credential.
                    prepared.credentials.take();
                    if let Some(completion) = prepared.completion.take()
                        && let Err(error) = completion.shutdown().await
                    {
                        eprintln!(
                            "host controller completion shutdown held connection={} reason={error}",
                            prepared.binding.connection_id
                        );
                    }
                }
            }
        }
        if remove_prepared {
            connections.remove(index);
        } else {
            index += 1;
        }
    }
    Ok(())
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
    let reserve = budget.admission_reserve();
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs,
                cpu_millicores,
                memory_bytes,
            },
            OperationalCapacityLimits {
                cpu_millicores: reserve.cpu_millicores,
                memory_bytes: reserve.memory_bytes,
            },
        )
        .await
}

async fn prepare_active_connections(
    journal: &Journal,
    config: &HostConfig,
    selected_engine: &SelectedEngineContext,
    docker: &Docker,
) -> Result<Vec<PreparedConnection>, ListenFault> {
    let mut prepared = Vec::with_capacity(config.connections().len());
    for binding in config.connections() {
        prepared.push(prepare_connection(journal, binding, config, selected_engine, docker).await?);
    }
    Ok(prepared)
}

async fn bind_connection(
    journal: &Journal,
    binding: &RepositoryConnection,
    config: &HostConfig,
    selected_engine: &SelectedEngineContext,
) -> Result<(), HostError> {
    let selected = EngineContext {
        engine_id: selected_engine.engine_id().to_owned(),
        endpoint: config.docker.endpoint.clone(),
        platform: config.docker.platform.clone(),
    };
    if !selected_engine.matches_selection(&selected) {
        return Err(HostError::Ownership);
    }
    let scoped = journal.connection_scope(binding).await?;
    scoped
        .bind_engine_context(
            selected_engine.engine_id(),
            &config.docker.endpoint,
            &config.docker.platform,
        )
        .await
}

struct PreparedConnection {
    binding: RepositoryConnection,
    scope: ConnectionJournal,
    set_id: i64,
    session_intent: i64,
    completion: Option<CompletionWorker>,
    credentials: Option<SessionCredentials>,
}

struct SessionCredentials {
    link: Link,
    session: QueueSession,
}

impl PreparedConnection {
    fn is_for(&self, binding: &RepositoryConnection) -> bool {
        self.binding == *binding
    }
}

async fn prepare_connection(
    journal: &Journal,
    binding: &RepositoryConnection,
    config: &HostConfig,
    selected_engine: &SelectedEngineContext,
    docker: &Docker,
) -> Result<PreparedConnection, ListenFault> {
    bind_connection(journal, binding, config, selected_engine).await?;
    let credential = crate::keychain::load_secret(
        binding.credential_ref.service(),
        binding.credential_ref.account(),
    )?;
    let pat = connection_pat(&credential)?;
    let ConnectionSetup {
        set,
        mut link,
        scoped,
    } = ensure_connection_scale_set(pat, binding, journal).await?;
    drop(credential);
    let stored = scoped.scale_set().await?.ok_or(HostError::Ownership)?;
    let source_binding = scoped.source_authorization_binding().await?;
    if stored.id != set.id
        || stored.name != set.name
        || stored.ownership != set.ownership
        || source_binding.scale_set_id() != set.id
    {
        return Err(HostError::Ownership.into());
    }
    let completion = CompletionWorker::start_scoped(
        scoped.clone(),
        docker.clone(),
        link.base(),
        link.token().to_owned(),
    )?;
    let (session, session_intent, _header_allocation) =
        open_connection_session(&mut link, set.id, &scoped).await?;
    Ok(PreparedConnection {
        binding: binding.clone(),
        scope: scoped,
        set_id: set.id,
        session_intent,
        completion: Some(completion),
        credentials: Some(SessionCredentials { link, session }),
    })
}

fn connection_pat(credential: &[u8]) -> Result<&str, HostError> {
    if credential.is_empty()
        || credential.len() > 4096
        || credential.iter().any(|byte| !byte.is_ascii_graphic())
    {
        return Err(HostError::Keychain);
    }
    str::from_utf8(credential).map_err(|_| HostError::Keychain)
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
    selected_engine: &'a SelectedEngineContext,
    docker: &'a Docker,
    connection_sessions: &'a mut Vec<PreparedConnection>,
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
        if self
            .connection_sessions
            .iter()
            .any(|prepared| prepared.is_for(binding))
        {
            return bind_connection(self.journal, binding, config, self.selected_engine).await;
        }
        let prepared = prepare_connection(
            self.journal,
            binding,
            config,
            self.selected_engine,
            self.docker,
        )
        .await
        .map_err(|_| HostError::Journal)?;
        self.connection_sessions.push(prepared);
        Ok(())
    }

    fn publish_config(&mut self, config: &HostConfig) -> Result<(), HostError> {
        self.control_state.replace_config(config)
    }

    async fn retire_removed(&mut self, binding: &RepositoryConnection) -> Result<bool, HostError> {
        super::retirement::retire_if_removed(self.state, self.journal, self.control_state, binding)
            .await
    }
}
