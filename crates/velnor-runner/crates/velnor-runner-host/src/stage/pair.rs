//! Prepare `DinD` before JIT. Start the runner only after durable preparation.

use std::collections::HashMap;
use std::time::Duration;

use bollard::Docker;
use bollard::query_parameters::RemoveContainerOptionsBuilder;
use tokio::time::{sleep, timeout};

use crate::action_archive_seed::ActionArchiveLease;
use crate::error::{HostError, PreparationCause};
use crate::launch_identity::LaunchIdentity;
use crate::worker::resources::refuse_existing;
use crate::worker::{
    CreateProjection, PreparedDind, ResourceBudget, Started, confirmed_not_found, create_only,
    create_owned_volumes, deliver_jit, dind_create_for_identity, join_dind_net, list_launch,
    probe_dind, remove_owned_volumes, runner_create_for_identity, start_id, verify_container,
    verify_engine,
};

use super::reconcile_worker;

const CLEANUP_DEADLINE: Duration = Duration::from_secs(45);
const READINESS_DEADLINE: Duration = Duration::from_secs(30);
const READINESS_STEP: Duration = Duration::from_millis(500);
const INSPECT_CALL: Duration = Duration::from_secs(5);
const PROBE_CALL: Duration = Duration::from_secs(15);
const CONTAINER_REMOVE_CALL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DindProbe {
    /// Inner daemon responds with the configured VFS root.
    Ready,
    /// The inner daemon is still starting.
    Starting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContainerRecord {
    pub(crate) id: String,
    pub(crate) labels: HashMap<String, String>,
    pub(crate) running: Option<bool>,
}

pub(crate) trait PairEngine {
    async fn prepare_volumes(&self, identity: &LaunchIdentity) -> Result<(), HostError>;
    async fn create(&self, spec: &CreateProjection) -> Result<String, HostError>;
    async fn start(&self, id: &str) -> Result<(), HostError>;
    async fn probe_dind(&self, id: &str) -> Result<DindProbe, HostError>;
    async fn list_launch(
        &self,
        identity: &LaunchIdentity,
    ) -> Result<Vec<ContainerRecord>, HostError>;
    async fn verify_container(
        &self,
        identity: &LaunchIdentity,
        role: &str,
        id: &str,
        dind_id: Option<&str>,
        archive_lease: Option<&ActionArchiveLease>,
        require_running: bool,
    ) -> Result<ContainerRecord, HostError>;
    /// Verify one container and check its Docker limits against `resource_budget`.
    ///
    /// Engines without budget plumbing fall back to [`Self::verify_container`].
    async fn verify_container_with_budget(
        &self,
        identity: &LaunchIdentity,
        role: &str,
        id: &str,
        dind_id: Option<&str>,
        archive_lease: Option<&ActionArchiveLease>,
        _resource_budget: ResourceBudget,
        require_running: bool,
    ) -> Result<ContainerRecord, HostError> {
        self.verify_container(identity, role, id, dind_id, archive_lease, require_running)
            .await
    }
    async fn write_jit(&self, id: &str, jit: &[u8]) -> Result<(), HostError>;
    async fn remove(&self, id: &str) -> Result<(), HostError>;
    async fn inspect_container(
        &self,
        id_or_name: &str,
    ) -> Result<Option<ContainerRecord>, HostError>;
    async fn remove_volumes(&self, identity: &LaunchIdentity) -> Result<(), HostError>;
    async fn verify_engine(&self, identity: &LaunchIdentity) -> Result<(), HostError>;
}

impl PairEngine for Docker {
    async fn prepare_volumes(&self, identity: &LaunchIdentity) -> Result<(), HostError> {
        create_owned_volumes(self, identity).await
    }

    async fn create(&self, spec: &CreateProjection) -> Result<String, HostError> {
        refuse_existing(self, spec).await?;
        create_only(self, spec).await
    }

    async fn start(&self, id: &str) -> Result<(), HostError> {
        start_id(self, id).await
    }

    async fn probe_dind(&self, id: &str) -> Result<DindProbe, HostError> {
        probe_dind(self, id).await
    }

    async fn list_launch(
        &self,
        identity: &LaunchIdentity,
    ) -> Result<Vec<ContainerRecord>, HostError> {
        list_launch(self, identity).await
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
        verify_container(
            self,
            identity,
            role,
            id,
            dind_id,
            archive_lease,
            None,
            require_running,
        )
        .await
    }

    async fn verify_container_with_budget(
        &self,
        identity: &LaunchIdentity,
        role: &str,
        id: &str,
        dind_id: Option<&str>,
        archive_lease: Option<&ActionArchiveLease>,
        resource_budget: ResourceBudget,
        require_running: bool,
    ) -> Result<ContainerRecord, HostError> {
        verify_container(
            self,
            identity,
            role,
            id,
            dind_id,
            archive_lease,
            Some(resource_budget),
            require_running,
        )
        .await
    }

    async fn write_jit(&self, id: &str, jit: &[u8]) -> Result<(), HostError> {
        deliver_jit(self, id, jit).await
    }

    async fn remove(&self, id: &str) -> Result<(), HostError> {
        let options = RemoveContainerOptionsBuilder::new().force(true).build();
        match timeout(
            CONTAINER_REMOVE_CALL,
            self.remove_container(id, Some(options)),
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) if confirmed_not_found(&error) => Ok(()),
            Err(_) | Ok(Err(_)) => {
                if confirm_container_absent(self, id).await? {
                    Ok(())
                } else {
                    Err(HostError::Cleanup)
                }
            }
        }
    }

    async fn inspect_container(
        &self,
        id_or_name: &str,
    ) -> Result<Option<ContainerRecord>, HostError> {
        let found = match timeout(INSPECT_CALL, self.inspect_container(id_or_name, None)).await {
            Err(_) => return Err(HostError::DockerTimeout),
            Ok(Err(error)) if confirmed_not_found(&error) => return Ok(None),
            Ok(Err(_)) => return Err(HostError::Docker),
            Ok(Ok(found)) => found,
        };
        let record = ContainerRecord {
            id: found
                .id
                .filter(|id| valid_container_id(id))
                .ok_or(HostError::Ownership)?,
            labels: found
                .config
                .and_then(|config| config.labels)
                .ok_or(HostError::Ownership)?,
            running: found.state.and_then(|state| state.running),
        };
        Ok(Some(record))
    }

    async fn remove_volumes(&self, identity: &LaunchIdentity) -> Result<(), HostError> {
        remove_owned_volumes(self, identity).await
    }

    async fn verify_engine(&self, identity: &LaunchIdentity) -> Result<(), HostError> {
        verify_engine(self, identity).await
    }
}

fn valid_container_id(id: &str) -> bool {
    (12..=64).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

async fn confirm_container_absent(docker: &Docker, id: &str) -> Result<bool, HostError> {
    match timeout(INSPECT_CALL, docker.inspect_container(id, None)).await {
        Err(_) => Err(HostError::DockerTimeout),
        Ok(Err(error)) if confirmed_not_found(&error) => Ok(true),
        Ok(Err(_)) => Err(HostError::Docker),
        Ok(Ok(_)) => Ok(false),
    }
}

/// Create volumes and `DinD`. Return only after its Docker API and private `VFS` root are ready.
///
/// # Errors
///
/// Returns [`HostError::PreparationFailedClean`] only before JIT and only after exact
/// local cleanup succeeds. Ambiguous create, start, or cleanup results remain uncertain.
pub(crate) async fn prepare_dind<E: PairEngine>(
    engine: &E,
    identity: &LaunchIdentity,
) -> Result<PreparedDind, HostError> {
    let observed = reconcile_worker(engine, identity, None, None, None).await?;
    if observed.runner_id().is_some() {
        return Err(HostError::Ownership);
    }
    if let Some(dind_id) = observed.dind_id() {
        let record = engine
            .verify_container(identity, "dind", dind_id, None, None, false)
            .await?;
        match record.running {
            Some(true) => {}
            Some(false) => engine.start(dind_id).await?,
            None => return Err(HostError::Ownership),
        }
        let prepared = PreparedDind::from_journal(identity, dind_id)?;
        return match wait_dind_ready(engine, dind_id).await {
            Ok(()) => Ok(prepared),
            Err(error) => {
                cleanup_prepared_dind(engine, &prepared)
                    .await
                    .map_err(|_| HostError::Cleanup)?;
                match preparation_cause(error) {
                    Some(cause) => Err(HostError::PreparationFailedClean(cause)),
                    None => Err(error),
                }
            }
        };
    }
    engine.prepare_volumes(identity).await?;
    let dind_id = engine.create(&dind_create_for_identity(identity)?).await?;
    engine.start(&dind_id).await?;
    match wait_dind_ready(engine, &dind_id).await {
        Ok(()) => PreparedDind::from_journal(identity, &dind_id),
        Err(error) => {
            let prepared = PreparedDind::from_journal(identity, &dind_id)?;
            cleanup_prepared_dind(engine, &prepared)
                .await
                .map_err(|_| HostError::Cleanup)?;
            match preparation_cause(error) {
                Some(cause) => Err(HostError::PreparationFailedClean(cause)),
                None => Err(error),
            }
        }
    }
}

/// Start the runner on a prepared, exact-identity `DinD` and write the JIT payload.
///
/// # Errors
///
/// Returns an uncertainty error when runner create, start, or JIT delivery may have
/// completed. The caller must retain the durable launch reservation.
pub(crate) async fn start_runner<E: PairEngine>(
    engine: &E,
    prepared: &PreparedDind,
    jit: &[u8],
    archive_lease: Option<&ActionArchiveLease>,
) -> Result<Started, HostError> {
    if jit.is_empty() {
        return Err(HostError::EmptyJit);
    }
    let identity = prepared.identity();
    if archive_lease.is_some_and(|lease| lease.launch_id() != identity.launch_id()) {
        return Err(HostError::Ownership);
    }
    let observed = reconcile_worker(engine, identity, None, Some(prepared.dind_id()), None).await?;
    if observed.dind_id() != Some(prepared.dind_id()) || observed.runner_id().is_some() {
        return Err(HostError::Ownership);
    }
    engine
        .verify_container(identity, "dind", prepared.dind_id(), None, None, true)
        .await?;
    let cache_path = archive_lease.map(ActionArchiveLease::cache_path);
    let runner = runner_create_for_identity(identity, cache_path)?;
    let spec = join_dind_net(runner, prepared.dind_id())?;
    let runner_id = engine.create(&spec).await?;
    engine.start(&runner_id).await?;
    engine.write_jit(&runner_id, jit).await?;
    Ok(Started {
        dind_id: prepared.dind_id().to_owned(),
        runner_id,
    })
}

/// Remove a prepared `DinD` only when no runner exists for its durable identity.
///
/// Call this only before JIT or after the caller proves that JIT created no runner.
///
/// # Errors
///
/// Returns an error when any exact owner or absence check fails. The caller must
/// retain the launch reservation until reconciliation resolves the state.
pub(crate) async fn cleanup_prepared_dind<E: PairEngine>(
    engine: &E,
    prepared: &PreparedDind,
) -> Result<(), HostError> {
    timeout(
        CLEANUP_DEADLINE,
        super::cleanup::cleanup_unstarted_dind(engine, prepared.identity(), prepared.dind_id()),
    )
    .await
    .map_err(|_| HostError::Cleanup)?
}

async fn wait_dind_ready<E: PairEngine>(engine: &E, id: &str) -> Result<(), HostError> {
    timeout(READINESS_DEADLINE, async {
        loop {
            match timeout(PROBE_CALL, engine.probe_dind(id)).await {
                Err(_) => return Err(HostError::DockerTimeout),
                Ok(Err(error)) => return Err(error),
                Ok(Ok(DindProbe::Ready)) => return Ok(()),
                Ok(Ok(DindProbe::Starting)) => sleep(READINESS_STEP).await,
            }
        }
    })
    .await
    .map_err(|_| HostError::DindReadiness)?
}

fn preparation_cause(error: HostError) -> Option<PreparationCause> {
    match error {
        HostError::Docker => Some(PreparationCause::Docker),
        HostError::DockerTimeout => Some(PreparationCause::DockerTimeout),
        HostError::DindReadiness => Some(PreparationCause::DindReadiness),
        HostError::DindStorage => Some(PreparationCause::DindStorage),
        _ => None,
    }
}
