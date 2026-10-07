//! Docker projections for the pinned Linux runner image profile.

use crate::HostError;
use crate::stage::PairStop;
use velnor_runner_docker_spec::{RunnerImageProfile, runner_plan_for_profile};

use super::{CreateProjection, Started, dind_mounts_for_profile, worker_labels};

/// Private `DinD` create for the pinned official Linux image profile.
///
/// The upstream entrypoint adds a TCP listener when called without a command.
/// Passing an explicit Unix-socket-only `dockerd` command keeps its API private.
/// The official runner and DIND mounts share the same workspace and externals
/// paths so Docker actions and job containers can bind those sources.
///
/// # Errors
///
/// Returns [`HostError::Config`] for an expired profile or
/// [`HostError::ForbiddenMount`] when the worker identity is invalid.
pub fn dind_create_for_profile(
    private_volume: &str,
    profile: &RunnerImageProfile,
) -> Result<CreateProjection, HostError> {
    let runner = runner_plan_for_profile(private_volume, profile)?;
    let mounts = dind_mounts_for_profile(private_volume)?;
    let mut labels = worker_labels(private_volume, "dind");
    labels.sort_unstable();
    Ok(CreateProjection {
        name: format!("{private_volume}-dind"),
        image: profile.dind_image().to_owned(),
        platform: runner.platform,
        readonly_rootfs: false,
        env: Vec::new(),
        cmd: vec![
            "dockerd".to_owned(),
            "--host=unix:///var/run/docker.sock".to_owned(),
            format!("--group={}", profile.dind_socket_group()),
        ],
        labels,
        mounts,
        image_mounts: Vec::new(),
        privileged: true,
        group_add: Vec::new(),
        security_opts: Vec::new(),
        open_stdin: false,
        network_mode: None,
    })
}

/// Start a job using one fresh, explicitly selected runner/DinD image profile.
///
/// No fallback to the legacy image occurs when the profile is stale or unknown.
///
/// # Errors
///
/// Returns [`HostError::EmptyJit`] without Docker side effects for empty input,
/// or the first profile, Docker, or cleanup error.
pub async fn start_pair_with_profile(
    docker: &::bollard::Docker,
    private_volume: &str,
    jit: &[u8],
    profile: &RunnerImageProfile,
) -> Result<Started, HostError> {
    let partial = Box::pin(crate::stage::start_pair_until_with_profile(
        docker,
        private_volume,
        jit,
        PairStop::Jit,
        profile,
    ))
    .await?;
    Ok(Started {
        dind_id: partial.dind_id.ok_or(HostError::Docker)?,
        runner_id: partial.runner_id.ok_or(HostError::Docker)?,
    })
}
