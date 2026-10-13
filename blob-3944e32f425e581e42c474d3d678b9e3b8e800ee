//! Stop a worker pair between Docker calls.
//!
//! `remove_recorded` deletes only when the name still resolves to the owned id.

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::query_parameters::RemoveContainerOptionsBuilder;

use crate::docker_client::docker_deadline;
use crate::docker_spec::{DeleteDecision, delete_decision, runner_plan};
use crate::error::HostError;
use crate::worker::{
    CreateProjection, ResourceBudget, WorkerVolumeRole, WorkerVolumeVerification,
    create_named_volumes, create_only, deliver_jit, dind_create, join_dind_net,
    remove_worker_volumes, runner_create, start_id, verify_worker_volume,
};

/// Where `start_pair_until` returns. Later steps are not started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairStop {
    /// Volumes exist. No container has been created.
    Volumes,
    /// `DinD` exists and is not started.
    DindCreated,
    /// `DinD` is running. The runner does not exist.
    DindStarted,
    /// The runner exists and is not started.
    RunnerCreated,
    /// The runner is running. JIT has not been written.
    RunnerStarted,
    /// Both containers are up and JIT was written.
    Jit,
}

/// Containers created before a stop. Absent means that step did not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialPair {
    /// `DinD` id, after create.
    pub dind_id: Option<String>,
    /// Runner id, after create.
    pub runner_id: Option<String>,
}

pub(crate) trait PairEngine {
    async fn prepare_volumes(&self, volume: &str) -> Result<(), HostError>;
    async fn create(&self, spec: &CreateProjection) -> Result<String, HostError>;
    async fn start(&self, id: &str) -> Result<(), HostError>;
    async fn write_jit(&self, id: &str, jit: &[u8]) -> Result<(), HostError>;
    async fn remove(&self, id: &str) -> Result<(), HostError>;
    async fn id_for_name(&self, name: &str) -> Result<Option<String>, HostError>;
    async fn worker_id_for_name(
        &self,
        name: &str,
        volume: &str,
        role: &str,
    ) -> Result<Option<String>, HostError>;
    async fn verify_volume(
        &self,
        worker: &str,
        role: WorkerVolumeRole,
    ) -> Result<WorkerVolumeVerification, HostError>;
    async fn remove_worker_volumes(&self, volume: &str) -> Result<bool, HostError>;
    async fn running(&self, id: &str) -> Result<bool, HostError>;
}

/// Records container ids before the next external start.
pub(crate) trait PairSink {
    async fn volume(&self, volume: &str) -> Result<(), HostError>;
    async fn dind(&self, id: &str) -> Result<(), HostError>;
    async fn runner(&self, id: &str) -> Result<(), HostError>;
}

/// Sink that does not record ids.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Forget;

#[expect(
    clippy::unused_async_trait_impl,
    reason = "sink matches the async trait and does not await"
)]
impl PairSink for Forget {
    async fn volume(&self, _volume: &str) -> Result<(), HostError> {
        Ok(())
    }

    async fn dind(&self, _id: &str) -> Result<(), HostError> {
        Ok(())
    }

    async fn runner(&self, _id: &str) -> Result<(), HostError> {
        Ok(())
    }
}

impl PairEngine for Docker {
    async fn prepare_volumes(&self, volume: &str) -> Result<(), HostError> {
        let mounts = crate::worker::dind_mounts(volume)?;
        Box::pin(docker_deadline(create_named_volumes(self, volume, &mounts))).await??;
        crate::work_owner::own_work_volume(self, &format!("{volume}-work")).await
    }

    async fn create(&self, spec: &CreateProjection) -> Result<String, HostError> {
        Box::pin(docker_deadline(create_only(self, spec))).await?
    }

    async fn start(&self, id: &str) -> Result<(), HostError> {
        Box::pin(docker_deadline(start_id(self, id))).await?
    }

    async fn write_jit(&self, id: &str, jit: &[u8]) -> Result<(), HostError> {
        Box::pin(docker_deadline(deliver_jit(self, id, jit))).await?
    }

    async fn remove(&self, id: &str) -> Result<(), HostError> {
        let options = RemoveContainerOptionsBuilder::new().force(true).build();
        Box::pin(docker_deadline(self.remove_container(id, Some(options))))
            .await?
            .map_err(|_| HostError::Docker)
    }

    async fn id_for_name(&self, name: &str) -> Result<Option<String>, HostError> {
        if name.is_empty() {
            return Err(HostError::Docker);
        }
        match Box::pin(docker_deadline(self.inspect_container(name, None))).await? {
            Ok(body) => body
                .id
                .filter(|id| !id.is_empty())
                .map(Some)
                .ok_or(HostError::Docker),
            Err(DockerError::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(None),
            Err(_) => Err(HostError::Docker),
        }
    }

    async fn worker_id_for_name(
        &self,
        name: &str,
        volume: &str,
        role: &str,
    ) -> Result<Option<String>, HostError> {
        crate::worker::worker_id_for_name(self, name, volume, role).await
    }

    async fn verify_volume(
        &self,
        worker: &str,
        role: WorkerVolumeRole,
    ) -> Result<WorkerVolumeVerification, HostError> {
        verify_worker_volume(self, worker, role).await
    }

    async fn remove_worker_volumes(&self, volume: &str) -> Result<bool, HostError> {
        remove_worker_volumes(self, volume).await
    }

    async fn running(&self, id: &str) -> Result<bool, HostError> {
        let response = Box::pin(docker_deadline(self.inspect_container(id, None))).await?;
        match crate::launch::classify_inspect(response) {
            Ok(running) => Ok(running),
            Err(_) => Err(HostError::Docker),
        }
    }
}

/// Create through `stop`, then return. `Jit` matches [`crate::worker::start_pair`].
///
/// # Errors
///
/// Returns [`HostError::EmptyJit`] before any create when `jit` is empty.
/// Returns [`HostError::Docker`] when a Docker call fails. A failed later step
/// removes only an id that still inspects as the one this call created.
pub async fn start_pair_until(
    docker: &Docker,
    private_volume: &str,
    resource_budget: ResourceBudget,
    jit: &[u8],
    stop: PairStop,
) -> Result<PartialPair, HostError> {
    Box::pin(drive(
        docker,
        private_volume,
        resource_budget,
        jit,
        stop,
        &Forget,
    ))
    .await
}

pub(crate) async fn drive<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    resource_budget: ResourceBudget,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
) -> Result<PartialPair, HostError> {
    if jit.is_empty() {
        return Err(HostError::EmptyJit);
    }
    let runner = runner_create(&runner_plan(private_volume)?, resource_budget)?;
    let dind = dind_create(private_volume, resource_budget)?;
    sink.volume(private_volume).await?;
    engine.prepare_volumes(private_volume).await?;
    if stop == PairStop::Volumes {
        return Ok(PartialPair::none());
    }
    let dind_id = engine.create(&dind).await?;
    if let Err(error) = sink.dind(&dind_id).await {
        return drop_id(engine, &dind_id, error).await;
    }
    if stop == PairStop::DindCreated {
        return Ok(PartialPair::dind(dind_id));
    }
    start_or_drop(engine, &dind_id).await?;
    if stop == PairStop::DindStarted {
        return Ok(PartialPair::dind(dind_id));
    }
    let spec = join_or_drop(engine, runner, &dind_id).await?;
    let runner_id = match engine.create(&spec).await {
        Ok(id) => id,
        Err(error) => drop_id(engine, &dind_id, error).await?,
    };
    if let Err(error) = sink.runner(&runner_id).await {
        return drop_both(engine, &dind_id, &runner_id, error).await;
    }
    if stop == PairStop::RunnerCreated {
        return Ok(PartialPair::both(dind_id, runner_id));
    }
    if let Err(error) = engine.start(&runner_id).await {
        return drop_both(engine, &dind_id, &runner_id, error).await;
    }
    if stop == PairStop::RunnerStarted {
        return Ok(PartialPair::both(dind_id, runner_id));
    }
    if let Err(error) = engine.write_jit(&runner_id, jit).await {
        return drop_both(engine, &dind_id, &runner_id, error).await;
    }
    Ok(PartialPair::both(dind_id, runner_id))
}

/// Inspect `name`, then delete `owned_id` only on [`DeleteDecision::Delete`].
///
/// # Errors
///
/// Returns [`HostError::Docker`] when the name is empty, its identity cannot be
/// established, or the owned container cannot be removed.
pub async fn remove_recorded(
    docker: &Docker,
    owned_id: &str,
    name: &str,
) -> Result<DeleteDecision, HostError> {
    decide(docker, owned_id, name).await
}

pub(crate) async fn decide<E: PairEngine + ?Sized>(
    engine: &E,
    owned_id: &str,
    name: &str,
) -> Result<DeleteDecision, HostError> {
    let observed = engine.id_for_name(name).await?;
    let decision = delete_decision(owned_id, observed.as_deref());
    if decision == DeleteDecision::Delete {
        engine.remove(owned_id).await?;
    }
    Ok(decision)
}

async fn start_or_drop<E: PairEngine>(engine: &E, id: &str) -> Result<(), HostError> {
    if let Err(error) = engine.start(id).await {
        return drop_id(engine, id, error).await;
    }
    Ok(())
}

async fn join_or_drop<E: PairEngine>(
    engine: &E,
    runner: CreateProjection,
    dind_id: &str,
) -> Result<CreateProjection, HostError> {
    match join_dind_net(runner, dind_id) {
        Ok(spec) => Ok(spec),
        Err(error) => drop_id(engine, dind_id, error).await,
    }
}

async fn drop_id<E: PairEngine, T>(engine: &E, id: &str, error: HostError) -> Result<T, HostError> {
    let _kept = decide(engine, id, id).await.err();
    Err(error)
}

async fn drop_both<E: PairEngine, T>(
    engine: &E,
    dind_id: &str,
    runner_id: &str,
    error: HostError,
) -> Result<T, HostError> {
    let _runner = decide(engine, runner_id, runner_id).await.err();
    drop_id(engine, dind_id, error).await
}

impl PartialPair {
    fn none() -> Self {
        Self {
            dind_id: None,
            runner_id: None,
        }
    }

    fn dind(dind_id: String) -> Self {
        Self {
            dind_id: Some(dind_id),
            runner_id: None,
        }
    }

    fn both(dind_id: String, runner_id: String) -> Self {
        Self {
            dind_id: Some(dind_id),
            runner_id: Some(runner_id),
        }
    }
}

#[cfg(test)]
mod tests;
