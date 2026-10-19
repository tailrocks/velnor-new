//! Stop a worker pair between Docker calls.
//!
//! `remove_recorded` deletes only when the name still resolves to the owned id.

use crate::config::RepositoryConnection;
use bollard::Docker;

use crate::docker_spec::{DeleteDecision, delete_decision, runner_plan};
use crate::error::HostError;
use crate::launch::{SelectedEngineContext, StartObservationWitness, VerifiedWorkerImages};
use crate::worker::guest_image::GuestImageFile;
use crate::worker::{
    CreateProjection, ResourceBudget, dind_create_verified, join_dind_net, runner_create_verified,
};

/// Where [`drive`] returns. Later steps are not started.
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

/// Per-admission evidence checked after image inspection and before each Docker effect.
#[derive(Debug, Clone)]
pub(crate) struct StageAdmission {
    observation: StartObservationWitness,
    sample_generation_id: String,
}

impl StageAdmission {
    pub(crate) fn from_observation(
        observation: &StartObservationWitness,
        sample_generation_id: &str,
    ) -> Self {
        Self {
            observation: observation.clone(),
            sample_generation_id: sample_generation_id.to_owned(),
        }
    }

    fn check_fresh(&self, expected: &SelectedEngineContext) -> Result<(), HostError> {
        if self.observation.is_fresh_for(
            expected.engine_id(),
            expected.docker_root_digest(),
            &self.sample_generation_id,
        ) {
            Ok(())
        } else {
            Err(HostError::Ownership)
        }
    }

    async fn verify<E: PairEngine + ?Sized>(
        &self,
        engine: &E,
        expected: &SelectedEngineContext,
        images: &VerifiedWorkerImages,
    ) -> Result<(), HostError> {
        self.check_fresh(expected)?;
        verify_worker_images(engine, expected, images).await?;
        self.check_fresh(expected)
    }

    #[cfg(test)]
    pub(crate) fn test_fixture_for(expected: &SelectedEngineContext) -> Self {
        let (observation, generation) = StartObservationWitness::test_fixture_for(
            expected.engine_id(),
            expected.docker_root_digest(),
            false,
        );
        Self::from_observation(&observation, &generation)
    }

    #[cfg(test)]
    pub(crate) fn test_stale_fixture_for(expected: &SelectedEngineContext) -> Self {
        let (observation, generation) = StartObservationWitness::test_fixture_for(
            expected.engine_id(),
            expected.docker_root_digest(),
            true,
        );
        Self::from_observation(&observation, &generation)
    }
}

mod engine;
#[cfg(test)]
pub(crate) use engine::Forget;
pub(crate) use engine::{PairEngine, PairSink};

pub(crate) async fn drive<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    resource_budget: ResourceBudget,
    jit: &[u8],
    stop: PairStop,
    connection: &RepositoryConnection,
    images: &VerifiedWorkerImages,
    expected_engine: &SelectedEngineContext,
    guest_image: &GuestImageFile,
    admission: StageAdmission,
    sink: &S,
) -> Result<PartialPair, HostError> {
    if jit.is_empty() {
        return Err(HostError::EmptyJit);
    }
    // Project and validate both containers before recording volumes or calling Docker.
    let plan = runner_plan(private_volume)?;
    let runner = runner_create_verified(&plan, resource_budget, connection, images, guest_image)?;
    let dind = dind_create_verified(private_volume, resource_budget, images)?;
    admission.verify(engine, expected_engine, images).await?;
    sink.volume(private_volume).await?;
    admission.check_fresh(expected_engine)?;
    engine
        .prepare_volumes(private_volume, &admission, expected_engine, images)
        .await?;
    if stop == PairStop::Volumes {
        return Ok(PartialPair::none());
    }
    admission.verify(engine, expected_engine, images).await?;
    let dind_id = engine.create(&dind).await?;
    if let Err(error) = sink.dind(&dind_id).await {
        return drop_id(engine, expected_engine, images, &admission, &dind_id, error).await;
    }
    if stop == PairStop::DindCreated {
        return Ok(PartialPair::dind(dind_id));
    }
    start_or_drop(engine, expected_engine, images, &admission, &dind_id).await?;
    drive_runner(
        engine,
        runner,
        dind_id,
        images,
        expected_engine,
        admission,
        stop,
        jit,
        sink,
    )
    .await
}

async fn drive_runner<E: PairEngine, S: PairSink>(
    engine: &E,
    runner: CreateProjection,
    dind_id: String,
    images: &VerifiedWorkerImages,
    expected_engine: &SelectedEngineContext,
    admission: StageAdmission,
    stop: PairStop,
    jit: &[u8],
    sink: &S,
) -> Result<PartialPair, HostError> {
    if stop == PairStop::DindStarted {
        return Ok(PartialPair::dind(dind_id));
    }
    let spec = join_or_drop(
        engine,
        expected_engine,
        images,
        &admission,
        runner,
        &dind_id,
    )
    .await?;
    admission.verify(engine, expected_engine, images).await?;
    let runner_id = match engine.create(&spec).await {
        Ok(id) => id,
        Err(error) => drop_id(engine, expected_engine, images, &admission, &dind_id, error).await?,
    };
    if let Err(error) = sink.runner(&runner_id).await {
        return drop_both(
            engine,
            expected_engine,
            images,
            &admission,
            &dind_id,
            &runner_id,
            error,
        )
        .await;
    }
    if stop == PairStop::RunnerCreated {
        return Ok(PartialPair::both(dind_id, runner_id));
    }
    admission.verify(engine, expected_engine, images).await?;
    if let Err(error) = engine.start(&runner_id).await {
        return drop_both(
            engine,
            expected_engine,
            images,
            &admission,
            &dind_id,
            &runner_id,
            error,
        )
        .await;
    }
    if stop == PairStop::RunnerStarted {
        return Ok(PartialPair::both(dind_id, runner_id));
    }
    admission.verify(engine, expected_engine, images).await?;
    if let Err(error) = engine.write_jit(&runner_id, jit).await {
        return drop_both(
            engine,
            expected_engine,
            images,
            &admission,
            &dind_id,
            &runner_id,
            error,
        )
        .await;
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

async fn verify_worker_images<E: PairEngine + ?Sized>(
    engine: &E,
    expected_engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
) -> Result<(), HostError> {
    if !expected_engine.matches_images(images) {
        return Err(HostError::Ownership);
    }
    engine.verify_worker_images(expected_engine, images).await
}

async fn start_or_drop<E: PairEngine>(
    engine: &E,
    expected_engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    admission: &StageAdmission,
    id: &str,
) -> Result<(), HostError> {
    admission.verify(engine, expected_engine, images).await?;
    if let Err(error) = engine.start(id).await {
        return drop_id(engine, expected_engine, images, admission, id, error).await;
    }
    Ok(())
}

async fn join_or_drop<E: PairEngine>(
    engine: &E,
    expected_engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    admission: &StageAdmission,
    runner: CreateProjection,
    dind_id: &str,
) -> Result<CreateProjection, HostError> {
    match join_dind_net(runner, dind_id) {
        Ok(spec) => Ok(spec),
        Err(error) => drop_id(engine, expected_engine, images, admission, dind_id, error).await,
    }
}

async fn drop_id<E: PairEngine, T>(
    engine: &E,
    expected_engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    admission: &StageAdmission,
    id: &str,
    error: HostError,
) -> Result<T, HostError> {
    let _kept = decide_verified(engine, expected_engine, images, admission, id, id)
        .await
        .err();
    Err(error)
}

async fn drop_both<E: PairEngine, T>(
    engine: &E,
    expected_engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    admission: &StageAdmission,
    dind_id: &str,
    runner_id: &str,
    error: HostError,
) -> Result<T, HostError> {
    let _runner = decide_verified(
        engine,
        expected_engine,
        images,
        admission,
        runner_id,
        runner_id,
    )
    .await
    .err();
    drop_id(engine, expected_engine, images, admission, dind_id, error).await
}

async fn decide_verified<E: PairEngine + ?Sized>(
    engine: &E,
    expected_engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    admission: &StageAdmission,
    owned_id: &str,
    name: &str,
) -> Result<DeleteDecision, HostError> {
    admission.verify(engine, expected_engine, images).await?;
    let observed = engine.id_for_name(name).await?;
    let decision = delete_decision(owned_id, observed.as_deref());
    if decision == DeleteDecision::Delete {
        admission.verify(engine, expected_engine, images).await?;
        engine.remove(owned_id).await?;
    }
    Ok(decision)
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
