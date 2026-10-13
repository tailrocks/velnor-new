//! Stop a worker pair between Docker calls.
//!
//! `remove_recorded` deletes only when the name still resolves to the owned id.

use bollard::Docker;

use crate::HostError;
use crate::docker_spec::{DeleteDecision, RunnerImageProfile, delete_decision, runner_plan};
use crate::worker::{CreateProjection, dind_create, join_dind_net, runner_create};

mod docker_engine;
mod profile;
pub use profile::{drive_with_profile, start_pair_until_with_profile};

/// Where `start_pair_until` returns. Later steps are not started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairStop {
    /// Volumes exist. No container has been created.
    Volumes,
    /// `DinD` exists and is not started.
    DindCreated,
    /// `DinD` is running. The profiled path may already have created the stopped runner.
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

/// Docker operations behind one worker pair. Async so fakes and the
/// engine share call sites; public for the launch crate.
#[expect(
    async_fn_in_trait,
    reason = "workspace style is async traits; RPITIT migration is a separate decision"
)]
pub trait PairEngine {
    /// Create the named volumes for `volume`.
    async fn prepare_volumes(&self, volume: &str) -> Result<(), HostError>;
    /// Create volumes for an explicit image profile. Existing engines keep the
    /// legacy path unless they override this method.
    async fn prepare_volumes_for_profile(
        &self,
        volume: &str,
        _profile: Option<&RunnerImageProfile>,
    ) -> Result<(), HostError> {
        self.prepare_volumes(volume).await
    }
    /// Create one container from `spec`. Returns its id.
    async fn create(&self, spec: &CreateProjection) -> Result<String, HostError>;
    /// Start container `id`.
    async fn start(&self, id: &str) -> Result<(), HostError>;
    /// Write `jit` into container `id`.
    async fn write_jit(&self, id: &str, jit: &[u8]) -> Result<(), HostError>;
    /// Remove container `id`.
    async fn remove(&self, id: &str) -> Result<(), HostError>;
    /// Id of the container named `name`, when present.
    async fn id_for_name(&self, name: &str) -> Result<Option<String>, HostError>;
    /// Id of the owned `role` container named `name`, when present.
    async fn worker_id_for_name(
        &self,
        name: &str,
        volume: &str,
        role: &str,
    ) -> Result<Option<String>, HostError>;
    /// Remove owned volumes for `volume`. Reports whether any existed.
    async fn remove_worker_volumes(&self, volume: &str) -> Result<bool, HostError>;
    /// Whether container `id` is running.
    async fn running(&self, id: &str) -> Result<bool, HostError>;
}

/// Records container ids before the next external start.
#[expect(
    async_fn_in_trait,
    reason = "workspace style is async traits; RPITIT migration is a separate decision"
)]
pub trait PairSink {
    /// Record the worker volume before any container exists.
    async fn volume(&self, volume: &str) -> Result<(), HostError>;
    /// Record the dind container id.
    async fn dind(&self, id: &str) -> Result<(), HostError>;
    /// Record the runner container id.
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
    jit: &[u8],
    stop: PairStop,
) -> Result<PartialPair, HostError> {
    Box::pin(drive(docker, private_volume, jit, stop, &Forget)).await
}

/// Stage one worker pair through `stop`, recording ids in `sink`.
///
/// # Errors
///
/// Returns [`HostError::EmptyJit`] when `jit` is empty, or the first
/// engine or sink failure. Partial pairs are dropped before returning.
pub async fn drive<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
) -> Result<PartialPair, HostError> {
    drive_inner(engine, private_volume, jit, stop, sink, None).await
}

/// Shared runner/DinD pair lifecycle used by both the legacy and explicit profile paths.
pub(super) async fn drive_inner<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
    profile: Option<&RunnerImageProfile>,
) -> Result<PartialPair, HostError> {
    if jit.is_empty() {
        return Err(HostError::EmptyJit);
    }
    let admission = if profile.is_some() {
        Some(crate::apparmor::verify_runner_profile()?)
    } else {
        None
    };
    drive_inner_admitted(
        engine,
        private_volume,
        jit,
        stop,
        sink,
        profile,
        admission.as_ref(),
    )
    .await
}

async fn drive_inner_admitted<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
    profile: Option<&RunnerImageProfile>,
    admission: Option<&crate::apparmor::RunnerProfileAdmission>,
) -> Result<PartialPair, HostError> {
    if profile.is_some() != admission.is_some() {
        return Err(HostError::Config);
    }
    if jit.is_empty() {
        return Err(HostError::EmptyJit);
    }
    if let Some(profile) = profile {
        return profile::drive_admitted(
            engine,
            private_volume,
            jit,
            stop,
            sink,
            profile,
            *admission.ok_or(HostError::Config)?,
        )
        .await;
    }
    let runner_plan = runner_plan(private_volume)?;
    let dind = dind_create(private_volume)?;
    let runner = runner_create(&runner_plan)?;
    sink.volume(private_volume).await?;
    engine
        .prepare_volumes_for_profile(private_volume, profile)
        .await?;
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

#[cfg(test)]
pub(super) async fn drive_with_profile_for_test<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
    profile: &RunnerImageProfile,
) -> Result<PartialPair, HostError> {
    let admission = crate::apparmor::test_runner_profile_admission();
    drive_inner_admitted(
        engine,
        private_volume,
        jit,
        stop,
        sink,
        Some(profile),
        Some(&admission),
    )
    .await
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
