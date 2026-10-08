//! Durable launch staging that preserves exact resources after uncertain replies.

use crate::HostError;
use crate::stage::{
    PairEngine, PairSink, PairStartFailure, PairStartPhase, PairStop, PartialPair,
    RunnerStartRequirement,
};
use crate::worker::{dind_create, join_dind_net, runner_create};
use velnor_runner_apparmor::RunnerProfileAdmission;
use velnor_runner_docker_spec::{RunnerImageProfile, runner_plan};

/// Create an immutable failure record without attempting cleanup.
pub(super) fn failure(
    error: HostError,
    partial: PartialPair,
    phase: PairStartPhase,
    side_effect_may_have_succeeded: bool,
) -> PairStartFailure {
    PairStartFailure::new(error, partial, phase, side_effect_may_have_succeeded)
}

/// Stage a legacy/macOS worker pair while preserving resources for journal recovery.
///
/// # Errors
///
/// Returns a `PairStartFailure` containing all known resource identities when
/// persistence, volume preparation, Docker, or JIT delivery fails.
pub async fn drive_with_sink<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
) -> Result<PartialPair, PairStartFailure> {
    drive_preserving(engine, private_volume, jit, stop, sink, None).await
}

/// Stage a profile-pinned Linux pair while preserving resources for journal recovery.
///
/// # Errors
///
/// Returns a `PairStartFailure` containing all known resource identities when
/// profile admission, persistence, volume, network, Docker, or JIT delivery fails.
pub async fn drive_with_profile_and_sink<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
    profile: &RunnerImageProfile,
) -> Result<PartialPair, PairStartFailure> {
    drive_preserving(engine, private_volume, jit, stop, sink, Some(profile)).await
}

pub(super) async fn drive_preserving<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
    profile: Option<&RunnerImageProfile>,
) -> Result<PartialPair, PairStartFailure> {
    if jit.is_empty() {
        return Err(failure(
            HostError::EmptyJit,
            PartialPair::none(),
            PairStartPhase::Preflight,
            false,
        ));
    }
    let admission = if profile.is_some() {
        Some(
            velnor_runner_apparmor::verify_runner_profile().map_err(|error| {
                failure(error, PartialPair::none(), PairStartPhase::Preflight, false)
            })?,
        )
    } else {
        None
    };
    drive_with_admission(
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

async fn drive_with_admission<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
    profile: Option<&RunnerImageProfile>,
    admission: Option<&RunnerProfileAdmission>,
) -> Result<PartialPair, PairStartFailure> {
    if profile.is_some() != admission.is_some() {
        return Err(failure(
            HostError::Config,
            PartialPair::none(),
            PairStartPhase::Preflight,
            false,
        ));
    }
    if jit.is_empty() {
        return Err(failure(
            HostError::EmptyJit,
            PartialPair::none(),
            PairStartPhase::Preflight,
            false,
        ));
    }
    if let Some(profile) = profile {
        let admission = admission.ok_or_else(|| {
            failure(
                HostError::Config,
                PartialPair::none(),
                PairStartPhase::Preflight,
                false,
            )
        })?;
        return super::profile::drive_admitted_preserving(
            engine,
            private_volume,
            jit,
            stop,
            sink,
            profile,
            *admission,
        )
        .await;
    }
    drive_legacy_preserving(engine, private_volume, jit, stop, sink).await
}

async fn drive_legacy_preserving<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
) -> Result<PartialPair, PairStartFailure> {
    let (dind, runner) = legacy_projections(private_volume)?;
    sink.volume(private_volume).await.map_err(|error| {
        failure(
            error,
            PartialPair::none(),
            PairStartPhase::VolumeIntent,
            false,
        )
    })?;
    engine
        .prepare_volumes_for_profile(private_volume, None)
        .await
        .map_err(|error| {
            failure(
                error,
                PartialPair::none(),
                PairStartPhase::VolumePreparation,
                true,
            )
        })?;
    if stop == PairStop::Volumes {
        return Ok(PartialPair::none());
    }
    let dind_id = create_legacy_dind(engine, sink, &dind).await?;
    if stop == PairStop::DindCreated {
        return Ok(PartialPair::dind(dind_id));
    }
    engine.start(&dind_id).await.map_err(|error| {
        failure(
            error,
            PartialPair::dind(dind_id.clone()),
            PairStartPhase::DindStart,
            true,
        )
    })?;
    if stop == PairStop::DindStarted {
        return Ok(PartialPair::dind(dind_id));
    }
    let runner_id = create_legacy_runner(engine, sink, runner, &dind_id).await?;
    if stop == PairStop::RunnerCreated {
        return Ok(PartialPair::both(dind_id, runner_id));
    }
    start_legacy_runner(engine, sink, &dind_id, &runner_id, jit, stop).await
}

fn legacy_projections(
    private_volume: &str,
) -> Result<
    (
        crate::worker::CreateProjection,
        crate::worker::CreateProjection,
    ),
    PairStartFailure,
> {
    let runner_plan = runner_plan(private_volume)
        .map_err(|error| failure(error, PartialPair::none(), PairStartPhase::Preflight, false))?;
    let dind = dind_create(private_volume)
        .map_err(|error| failure(error, PartialPair::none(), PairStartPhase::Preflight, false))?;
    let runner = runner_create(&runner_plan)
        .map_err(|error| failure(error, PartialPair::none(), PairStartPhase::Preflight, false))?;
    Ok((dind, runner))
}

async fn create_legacy_dind<E: PairEngine, S: PairSink>(
    engine: &E,
    sink: &S,
    projection: &crate::worker::CreateProjection,
) -> Result<String, PairStartFailure> {
    let dind_id = engine.create(projection).await.map_err(|error| {
        failure(
            error,
            PartialPair::none(),
            PairStartPhase::DindCreation,
            true,
        )
    })?;
    sink.dind(&dind_id).await.map_err(|error| {
        failure(
            error,
            PartialPair::dind(dind_id.clone()),
            PairStartPhase::DindIdentity,
            true,
        )
    })?;
    Ok(dind_id)
}

async fn create_legacy_runner<E: PairEngine, S: PairSink>(
    engine: &E,
    sink: &S,
    projection: crate::worker::CreateProjection,
    dind_id: &str,
) -> Result<String, PairStartFailure> {
    let spec = join_dind_net(projection, dind_id).map_err(|error| {
        failure(
            error,
            PartialPair::dind(dind_id.to_owned()),
            PairStartPhase::RunnerProjection,
            true,
        )
    })?;
    let runner_id = engine.create(&spec).await.map_err(|error| {
        failure(
            error,
            PartialPair::dind(dind_id.to_owned()),
            PairStartPhase::RunnerCreation,
            true,
        )
    })?;
    sink.runner(&runner_id).await.map_err(|error| {
        failure(
            error,
            PartialPair::both(dind_id.to_owned(), runner_id.clone()),
            PairStartPhase::RunnerIdentity,
            true,
        )
    })?;
    Ok(runner_id)
}

async fn start_legacy_runner<E: PairEngine, S: PairSink>(
    engine: &E,
    sink: &S,
    dind_id: &str,
    runner_id: &str,
    jit: &[u8],
    stop: PairStop,
) -> Result<PartialPair, PairStartFailure> {
    let pair = || PartialPair::both(dind_id.to_owned(), runner_id.to_owned());
    sink.before_runner_start(runner_id, RunnerStartRequirement::LegacyCompatible)
        .await
        .map_err(|error| failure(error, pair(), PairStartPhase::RunnerStartIntent, true))?;
    engine
        .start(runner_id)
        .await
        .map_err(|error| failure(error, pair(), PairStartPhase::RunnerStart, true))?;
    if stop == PairStop::RunnerStarted {
        return Ok(pair());
    }
    engine
        .write_jit(runner_id, jit)
        .await
        .map_err(|error| failure(error, pair(), PairStartPhase::JitWrite, true))?;
    Ok(pair())
}

#[cfg(test)]
pub(super) async fn drive_profile_for_test<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
    profile: &RunnerImageProfile,
) -> Result<PartialPair, HostError> {
    let admission = velnor_runner_apparmor::test_runner_profile_admission();
    match drive_with_admission(
        engine,
        private_volume,
        jit,
        stop,
        sink,
        Some(profile),
        Some(&admission),
    )
    .await
    {
        Ok(pair) => Ok(pair),
        Err(failure) => {
            super::cleanup_partial(engine, failure.partial()).await;
            Err(failure.error())
        }
    }
}

#[cfg(test)]
pub(super) async fn drive_profile_preserving_for_test<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
    profile: &RunnerImageProfile,
) -> Result<PartialPair, PairStartFailure> {
    let admission = velnor_runner_apparmor::test_runner_profile_admission();
    drive_with_admission(
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
