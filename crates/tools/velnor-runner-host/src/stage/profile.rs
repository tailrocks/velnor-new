//! Explicit Linux image-profile staging entry points.

use bollard::Docker;

use crate::docker_spec::RunnerImageProfile;
use crate::error::HostError;

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
