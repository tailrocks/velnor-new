//! Immutable Linux image-profile entry points and their Docker projection.

use bollard::Docker;

use crate::HostError;
use crate::worker::{
    CreateProjection, WorkerNetworkPlan, dind_create_for_profile, join_dind_net, runner_create,
};
use velnor_runner_apparmor::RunnerProfileAdmission;
use velnor_runner_docker_spec::{RunnerImageProfile, runner_plan_for_profile};

use super::preserving::failure;
use super::{
    Forget, PairEngine, PairSink, PairStartFailure, PairStartPhase, PairStop, PartialPair,
};

/// Stage a profile-pinned pair through the requested stop.
///
/// # Errors
///
/// Returns an error for empty JIT, a stale profile, or a failed Docker step.
/// The compatibility facade has no durable sink and therefore fails before
/// creating profile-path resources.
pub async fn start_pair_until_with_profile(
    docker: &Docker,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    profile: &RunnerImageProfile,
) -> Result<PartialPair, HostError> {
    super::preserving::drive_with_profile_and_sink(
        docker,
        private_volume,
        jit,
        stop,
        &Forget,
        profile,
    )
    .await
    .map_err(|failure| failure.error())
}

/// Stage a profile-pinned pair through the requested stop.
///
/// The durable launch API is [`super::preserving::drive_with_profile_and_sink`].
/// This compatibility facade maps errors only; it never deletes resources.
///
/// # Errors
///
/// Returns the first profile, engine, or sink error.
pub async fn drive_with_profile<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
    profile: &RunnerImageProfile,
) -> Result<PartialPair, HostError> {
    super::preserving::drive_with_profile_and_sink(engine, private_volume, jit, stop, sink, profile)
        .await
        .map_err(|failure| failure.error())
}

pub(super) async fn drive_admitted_preserving<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
    profile: &RunnerImageProfile,
    _admission: RunnerProfileAdmission,
) -> Result<PartialPair, PairStartFailure> {
    let runner = profile_runner_projection(private_volume, profile)?;
    let dind = dind_create_for_profile(private_volume, profile)
        .map_err(|error| failure(error, PartialPair::none(), PairStartPhase::Preflight, false))?;
    sink.volume(private_volume).await.map_err(|error| {
        failure(
            error,
            PartialPair::none(),
            PairStartPhase::VolumeIntent,
            false,
        )
    })?;
    if stop == PairStop::Volumes {
        engine
            .prepare_volumes_for_profile(private_volume, Some(profile))
            .await
            .map_err(|error| {
                failure(
                    error,
                    PartialPair::none(),
                    PairStartPhase::VolumePreparation,
                    true,
                )
            })?;
        return Ok(PartialPair::none());
    }

    let network = WorkerNetworkPlan::for_worker(private_volume)
        .map_err(|error| failure(error, PartialPair::none(), PairStartPhase::Preflight, false))?;
    let network_id = provision_profile_network(engine, sink, &network).await?;
    engine
        .prepare_volumes_for_profile(private_volume, Some(profile))
        .await
        .map_err(|error| {
            failure(
                error,
                PartialPair::network(network_id.clone()),
                PairStartPhase::VolumePreparation,
                true,
            )
        })?;

    let dind_id = create_profile_dind(engine, sink, &dind, &network_id).await?;
    if stop == PairStop::DindCreated {
        return Ok(PartialPair::dind_with_network(network_id, dind_id));
    }

    let runner_id = create_profile_runner(engine, sink, runner, &network_id, &dind_id).await?;
    if stop == PairStop::RunnerCreated {
        return Ok(PartialPair::both_with_network(
            network_id, dind_id, runner_id,
        ));
    }

    start_profile_pair(engine, sink, &network_id, &dind_id, &runner_id, jit, stop).await
}

fn profile_runner_projection(
    private_volume: &str,
    profile: &RunnerImageProfile,
) -> Result<CreateProjection, PairStartFailure> {
    let plan = runner_plan_for_profile(private_volume, profile)
        .map_err(|error| failure(error, PartialPair::none(), PairStartPhase::Preflight, false))?;
    runner_create(&plan)
        .map_err(|error| failure(error, PartialPair::none(), PairStartPhase::Preflight, false))
}

async fn provision_profile_network<E: PairEngine, S: PairSink>(
    engine: &E,
    sink: &S,
    network: &WorkerNetworkPlan,
) -> Result<String, PairStartFailure> {
    sink.outer_network_intent(network.name())
        .await
        .map_err(|error| {
            failure(
                error,
                PartialPair::none(),
                PairStartPhase::NetworkIntent,
                false,
            )
        })?;
    let network_id = match engine.ensure_worker_network(network).await {
        Ok(network_id) if WorkerNetworkPlan::accepts_id(&network_id) => network_id,
        Ok(_) => {
            return Err(failure(
                HostError::Docker,
                PartialPair::none(),
                PairStartPhase::NetworkCreation,
                true,
            ));
        }
        Err(network_failure) => {
            let partial = network_failure
                .network_id()
                .map_or_else(PartialPair::none, |id| PartialPair::network(id.to_owned()));
            if let Some(network_id) = network_failure.network_id()
                && let Err(error) = sink.outer_network(network_id).await
            {
                return Err(failure(
                    error,
                    partial,
                    PairStartPhase::NetworkIdentity,
                    true,
                ));
            }
            return Err(failure(
                network_failure.error(),
                partial,
                PairStartPhase::NetworkCreation,
                network_failure.side_effect_may_have_succeeded(),
            ));
        }
    };
    let network_partial = PartialPair::network(network_id.clone());
    sink.outer_network(&network_id).await.map_err(|error| {
        failure(
            error,
            network_partial.clone(),
            PairStartPhase::NetworkIdentity,
            true,
        )
    })?;
    Ok(network_id)
}

async fn create_profile_dind<E: PairEngine, S: PairSink>(
    engine: &E,
    sink: &S,
    projection: &CreateProjection,
    network_id: &str,
) -> Result<String, PairStartFailure> {
    let network_partial = || PartialPair::network(network_id.to_owned());
    let dind_id = engine
        .create(projection)
        .await
        .map_err(|error| failure(error, network_partial(), PairStartPhase::DindCreation, true))?;
    let partial_dind = PartialPair::dind_with_network(network_id.to_owned(), dind_id.clone());
    if !crate::worker::dind_container_id(&dind_id) {
        return Err(failure(
            HostError::Docker,
            partial_dind,
            PairStartPhase::DindIdentity,
            true,
        ));
    }
    sink.dind(&dind_id).await.map_err(|error| {
        failure(
            error,
            partial_dind.clone(),
            PairStartPhase::DindIdentity,
            true,
        )
    })?;
    Ok(dind_id)
}

async fn create_profile_runner<E: PairEngine, S: PairSink>(
    engine: &E,
    sink: &S,
    projection: CreateProjection,
    network_id: &str,
    dind_id: &str,
) -> Result<String, PairStartFailure> {
    let partial_dind = || PartialPair::dind_with_network(network_id.to_owned(), dind_id.to_owned());
    let runner = join_dind_net(projection, dind_id).map_err(|error| {
        failure(
            error,
            partial_dind(),
            PairStartPhase::RunnerProjection,
            true,
        )
    })?;
    let runner_id = engine
        .create(&runner)
        .await
        .map_err(|error| failure(error, partial_dind(), PairStartPhase::RunnerCreation, true))?;
    let partial_pair = PartialPair::both_with_network(
        network_id.to_owned(),
        dind_id.to_owned(),
        runner_id.clone(),
    );
    if !crate::worker::dind_container_id(&runner_id) {
        return Err(failure(
            HostError::Docker,
            partial_pair,
            PairStartPhase::RunnerIdentity,
            true,
        ));
    }
    sink.runner(&runner_id).await.map_err(|error| {
        failure(
            error,
            partial_pair.clone(),
            PairStartPhase::RunnerIdentity,
            true,
        )
    })?;
    Ok(runner_id)
}

async fn start_profile_pair<E: PairEngine, S: PairSink>(
    engine: &E,
    sink: &S,
    network_id: &str,
    dind_id: &str,
    runner_id: &str,
    jit: &[u8],
    stop: PairStop,
) -> Result<PartialPair, PairStartFailure> {
    let partial_pair = || {
        PartialPair::both_with_network(
            network_id.to_owned(),
            dind_id.to_owned(),
            runner_id.to_owned(),
        )
    };
    engine
        .start(dind_id)
        .await
        .map_err(|error| failure(error, partial_pair(), PairStartPhase::DindStart, true))?;
    if stop == PairStop::DindStarted {
        return Ok(partial_pair());
    }
    sink.before_runner_start(runner_id, super::RunnerStartRequirement::DurableRequired)
        .await
        .map_err(|error| {
            failure(
                error,
                partial_pair(),
                PairStartPhase::RunnerStartIntent,
                true,
            )
        })?;
    engine
        .start(runner_id)
        .await
        .map_err(|error| failure(error, partial_pair(), PairStartPhase::RunnerStart, true))?;
    if stop == PairStop::RunnerStarted {
        return Ok(partial_pair());
    }
    engine
        .write_jit(runner_id, jit)
        .await
        .map_err(|error| failure(error, partial_pair(), PairStartPhase::JitWrite, true))?;
    Ok(partial_pair())
}
