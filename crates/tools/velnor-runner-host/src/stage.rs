//! Stop a worker pair between Docker calls.
//!
//! `remove_recorded` deletes only when the name still resolves to the owned id.

use bollard::Docker;

use crate::HostError;
use crate::worker::{CreateProjection, WorkerNetworkFailure, WorkerNetworkPlan};
use velnor_runner_docker_spec::{DeleteDecision, RunnerImageProfile, delete_decision};

mod docker_engine;
mod preserving;
mod profile;
mod sink;
pub use preserving::{drive_with_profile_and_sink, drive_with_sink};
pub use profile::{drive_with_profile, start_pair_until_with_profile};
pub(crate) use sink::Forget;
pub use sink::{PairSink, PairStartFailure, PairStartPhase, RunnerStartRequirement};

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
    /// Linux outer bridge id, after create. Absent on the legacy/macOS path.
    pub outer_network_id: Option<String>,
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
    /// Find or create the exact persisted outer bridge for a Linux generation.
    async fn ensure_worker_network(
        &self,
        _plan: &WorkerNetworkPlan,
    ) -> Result<String, WorkerNetworkFailure> {
        Err(WorkerNetworkFailure::new(HostError::Config, None, false))
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
    drive_inner(engine, private_volume, jit, stop, sink).await
}

/// Shared runner/DinD pair lifecycle used by both the legacy and explicit profile paths.
pub(super) async fn drive_inner<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
) -> Result<PartialPair, HostError> {
    match preserving::drive_preserving(engine, private_volume, jit, stop, sink, None).await {
        Ok(pair) => Ok(pair),
        Err(failure) => {
            cleanup_partial(engine, failure.partial()).await;
            Err(failure.error())
        }
    }
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
    preserving::drive_profile_for_test(engine, private_volume, jit, stop, sink, profile).await
}

#[cfg(test)]
pub(super) async fn drive_with_profile_and_sink_for_test<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
    profile: &RunnerImageProfile,
) -> Result<PartialPair, PairStartFailure> {
    preserving::drive_profile_preserving_for_test(engine, private_volume, jit, stop, sink, profile)
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

pub(super) async fn cleanup_partial<E: PairEngine>(engine: &E, pair: &PartialPair) {
    if let Some(runner_id) = pair.runner_id.as_deref() {
        let _result = decide(engine, runner_id, runner_id).await;
    }
    if let Some(dind_id) = pair.dind_id.as_deref() {
        let _result = decide(engine, dind_id, dind_id).await;
    }
}

impl PartialPair {
    fn none() -> Self {
        Self {
            outer_network_id: None,
            dind_id: None,
            runner_id: None,
        }
    }

    fn dind(dind_id: String) -> Self {
        Self {
            outer_network_id: None,
            dind_id: Some(dind_id),
            runner_id: None,
        }
    }

    fn both(dind_id: String, runner_id: String) -> Self {
        Self {
            outer_network_id: None,
            dind_id: Some(dind_id),
            runner_id: Some(runner_id),
        }
    }

    fn network(network_id: String) -> Self {
        Self {
            outer_network_id: Some(network_id),
            ..Self::none()
        }
    }

    fn dind_with_network(network_id: String, dind_id: String) -> Self {
        Self {
            outer_network_id: Some(network_id),
            dind_id: Some(dind_id),
            runner_id: None,
        }
    }

    fn both_with_network(network_id: String, dind_id: String, runner_id: String) -> Self {
        Self {
            outer_network_id: Some(network_id),
            dind_id: Some(dind_id),
            runner_id: Some(runner_id),
        }
    }
}

#[cfg(test)]
mod tests;
