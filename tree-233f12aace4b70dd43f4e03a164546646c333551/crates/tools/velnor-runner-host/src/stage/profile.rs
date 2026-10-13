//! Explicit Linux image-profile staging entry points.

use bollard::Docker;

use crate::HostError;
use crate::docker_spec::{RunnerImageProfile, runner_plan_for_profile};
use crate::worker::{dind_create_for_profile, join_dind_net, runner_create};
use velnor_runner_apparmor::RunnerProfileAdmission;

use super::{Forget, PairEngine, PairSink, PairStop, PartialPair, drive_inner};

/// Stage the profile-pinned Linux runner/DinD pair through `stop`.
///
/// # Errors
///
/// Returns [`HostError::EmptyJit`] when `jit` is empty and rejects a stale
/// profile before Docker side effects.
pub async fn start_pair_until_with_profile(
    docker: &Docker,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    profile: &RunnerImageProfile,
) -> Result<PartialPair, HostError> {
    drive_inner(docker, private_volume, jit, stop, &Forget, Some(profile)).await
}

/// Stage a runner/DinD pair with an explicit immutable image profile.
///
/// The legacy macOS path remains in [`super::drive`]. This function never
/// falls back to it when the profile is invalid or stale.
///
/// # Errors
///
/// Returns the first engine or sink error. A partial pair is removed before
/// the error is returned.
pub async fn drive_with_profile<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
    profile: &RunnerImageProfile,
) -> Result<PartialPair, HostError> {
    drive_inner(engine, private_volume, jit, stop, sink, Some(profile)).await
}

pub(super) async fn drive_admitted<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    jit: &[u8],
    stop: PairStop,
    sink: &S,
    profile: &RunnerImageProfile,
    _admission: RunnerProfileAdmission,
) -> Result<PartialPair, HostError> {
    // Creating the runner while stopped seeds the empty `externals` volume
    // before the private Docker daemon can expose that path to job containers.
    let runner = runner_create(&runner_plan_for_profile(private_volume, profile)?)?;
    let dind = dind_create_for_profile(private_volume, profile)?;
    sink.volume(private_volume).await?;
    engine
        .prepare_volumes_for_profile(private_volume, Some(profile))
        .await?;
    if stop == PairStop::Volumes {
        return Ok(PartialPair::none());
    }

    let dind_id = engine.create(&dind).await?;
    if let Err(error) = sink.dind(&dind_id).await {
        return super::drop_id(engine, &dind_id, error).await;
    }
    if stop == PairStop::DindCreated {
        return Ok(PartialPair::dind(dind_id));
    }

    let runner = match join_dind_net(runner, &dind_id) {
        Ok(runner) => runner,
        Err(error) => return super::drop_id(engine, &dind_id, error).await,
    };
    let runner_id = match engine.create(&runner).await {
        Ok(id) => id,
        Err(error) => return super::drop_id(engine, &dind_id, error).await,
    };
    if let Err(error) = sink.runner(&runner_id).await {
        return super::drop_both(engine, &dind_id, &runner_id, error).await;
    }
    if stop == PairStop::RunnerCreated {
        return Ok(PartialPair::both(dind_id, runner_id));
    }

    if let Err(error) = engine.start(&dind_id).await {
        return super::drop_both(engine, &dind_id, &runner_id, error).await;
    }
    if stop == PairStop::DindStarted {
        return Ok(PartialPair::both(dind_id, runner_id));
    }
    if let Err(error) = engine.start(&runner_id).await {
        return super::drop_both(engine, &dind_id, &runner_id, error).await;
    }
    if stop == PairStop::RunnerStarted {
        return Ok(PartialPair::both(dind_id, runner_id));
    }
    if let Err(error) = engine.write_jit(&runner_id, jit).await {
        return super::drop_both(engine, &dind_id, &runner_id, error).await;
    }
    Ok(PartialPair::both(dind_id, runner_id))
}
