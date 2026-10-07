//! Immutable official runner and private `DinD` image profiles.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::HostError;
use velnor_runner_core::runner_work_path;

use super::{ContainerPlan, Mount, RUNNER_PLATFORM, private_volume_name};

const EXTERNALS_TARGET: &str = "/home/runner/externals";
const OFFICIAL_SOCKET_TARGET: &str = "/run/docker";
const APPARMOR_RUNNER: &str = "apparmor=velnor-runner";
const OFFICIAL_RUNNER_IMAGE: &str = "ghcr.io/actions/actions-runner@sha256:660f7b9d1e0007274f7c867e220b3c382137ef5a30d8e501a025ad50dbd5fb9d";
const OFFICIAL_DIND_IMAGE: &str = "docker.io/library/docker@sha256:dcac6f16dc25ddec91e2d467605775b95a035ab884b94cb4c2cc7cbef6fd726d";

/// One immutable runner/DinD image pair accepted by the Linux backend.
///
/// Fields are private so configuration can select a profile key but cannot inject
/// an arbitrary image, digest, architecture, UID, or security option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerImageProfile {
    key: &'static str,
    scale_set_name: &'static str,
    platform: &'static str,
    runner_image: &'static str,
    runner_manifest_digest: &'static str,
    runner_index_digest: &'static str,
    runner_config_digest: &'static str,
    runner_os: &'static str,
    runner_version: &'static str,
    runner_release_published_at: &'static str,
    runner_requalify_by: &'static str,
    runner_requalify_by_unix: u64,
    dind_image: &'static str,
    dind_manifest_digest: &'static str,
    dind_index_digest: &'static str,
    dind_config_digest: &'static str,
    dind_version: &'static str,
    dind_source: &'static str,
    dind_entrypoint_sha256: &'static str,
    runner_uid: u32,
    runner_gid: u32,
    runner_docker_gid: u32,
    dind_socket_group: &'static str,
    dind_socket_gid: u32,
}

impl RunnerImageProfile {
    /// Stable configuration key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        self.key
    }

    /// Scale Set selector this exact profile supports.
    #[must_use]
    pub const fn scale_set_name(self) -> &'static str {
        self.scale_set_name
    }

    /// OCI platform used for both immutable image manifests.
    #[must_use]
    pub const fn platform(self) -> &'static str {
        self.platform
    }

    /// Official runner image pinned to the linux/amd64 child manifest.
    #[must_use]
    pub const fn runner_image(self) -> &'static str {
        self.runner_image
    }

    /// Immutable platform-specific OCI manifest digest for the runner image.
    #[must_use]
    pub const fn runner_manifest_digest(self) -> &'static str {
        self.runner_manifest_digest
    }

    /// Multi-platform OCI index digest recorded for image provenance.
    #[must_use]
    pub const fn runner_index_digest(self) -> &'static str {
        self.runner_index_digest
    }

    /// Runner OCI image-config digest recorded for image provenance.
    #[must_use]
    pub const fn runner_config_digest(self) -> &'static str {
        self.runner_config_digest
    }

    /// Ubuntu release reported by the official image metadata.
    #[must_use]
    pub const fn runner_os(self) -> &'static str {
        self.runner_os
    }

    /// Runner release version contained in the pinned image.
    #[must_use]
    pub const fn runner_version(self) -> &'static str {
        self.runner_version
    }

    /// Upstream Actions runner release publication timestamp.
    #[must_use]
    pub const fn runner_release_published_at(self) -> &'static str {
        self.runner_release_published_at
    }

    /// Deadline at which the runner profile becomes stale.
    #[must_use]
    pub const fn runner_requalify_by(self) -> &'static str {
        self.runner_requalify_by
    }

    /// Official Docker `DinD` image pinned to its `linux/amd64` child manifest.
    #[must_use]
    pub const fn dind_image(self) -> &'static str {
        self.dind_image
    }

    /// Immutable platform-specific OCI manifest digest for the `DinD` image.
    #[must_use]
    pub const fn dind_manifest_digest(self) -> &'static str {
        self.dind_manifest_digest
    }

    /// Multi-platform OCI index digest recorded for image provenance.
    #[must_use]
    pub const fn dind_index_digest(self) -> &'static str {
        self.dind_index_digest
    }

    /// `DinD` OCI image-config digest recorded for image provenance.
    #[must_use]
    pub const fn dind_config_digest(self) -> &'static str {
        self.dind_config_digest
    }

    /// Docker Engine release in the pinned `DinD` image.
    #[must_use]
    pub const fn dind_version(self) -> &'static str {
        self.dind_version
    }

    /// Immutable source revision for the official `DinD` entrypoint.
    #[must_use]
    pub const fn dind_source(self) -> &'static str {
        self.dind_source
    }

    /// SHA-256 of the pinned official `dockerd-entrypoint.sh` source.
    #[must_use]
    pub const fn dind_entrypoint_sha256(self) -> &'static str {
        self.dind_entrypoint_sha256
    }

    /// Numeric user configured by the official runner image.
    #[must_use]
    pub const fn runner_uid(self) -> u32 {
        self.runner_uid
    }

    /// Numeric primary group configured by the official runner image.
    #[must_use]
    pub const fn runner_gid(self) -> u32 {
        self.runner_gid
    }

    /// Numeric Docker group configured by the official runner image.
    #[must_use]
    pub const fn runner_docker_gid(self) -> u32 {
        self.runner_docker_gid
    }

    /// Group name configured by the official `DinD` image for its Unix socket.
    #[must_use]
    pub const fn dind_socket_group(self) -> &'static str {
        self.dind_socket_group
    }

    /// Numeric group id configured by the official `DinD` image for its Unix socket.
    #[must_use]
    pub const fn dind_socket_gid(self) -> u32 {
        self.dind_socket_gid
    }

    fn ensure_fresh(self, now: SystemTime) -> Result<(), HostError> {
        let seconds = now
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Config)?
            .as_secs();
        if seconds >= self.runner_requalify_by_unix {
            return Err(HostError::Config);
        }
        Ok(())
    }
}

const RUNNER_PROFILE: RunnerImageProfile = RunnerImageProfile {
    key: "ubuntu-24.04-amd64",
    scale_set_name: "ubuntu-24.04-scale-set",
    platform: RUNNER_PLATFORM,
    runner_image: OFFICIAL_RUNNER_IMAGE,
    runner_manifest_digest: "sha256:660f7b9d1e0007274f7c867e220b3c382137ef5a30d8e501a025ad50dbd5fb9d",
    runner_index_digest: "sha256:4ffadc0002b2581327e06101fc8c06cd189232baf79fe561fac9caeb76f5e807",
    runner_config_digest: "sha256:3027ff446b2083fc96c214916922b117f4d996b0683295ce7b43fd0048eae210",
    runner_os: "ubuntu24",
    runner_version: "2.338.0",
    runner_release_published_at: "2026-10-06T13:55:11Z",
    runner_requalify_by: "2026-11-05T13:55:11Z",
    runner_requalify_by_unix: 1_793_886_911,
    dind_image: OFFICIAL_DIND_IMAGE,
    dind_manifest_digest: "sha256:dcac6f16dc25ddec91e2d467605775b95a035ab884b94cb4c2cc7cbef6fd726d",
    dind_index_digest: "sha256:7dcdfc4a20246236f558175182ccace1eb15a41bd3eb119dd2284f393498b7c1",
    dind_config_digest: "sha256:a32a1e3b62afa34576d728f5b65075e89e468ebabc56696df28fa256e8ac0b5d",
    dind_version: "29.8.2",
    dind_source: "docker-library/official-images@a888e7fd9fd891fe0d6050620b2b9fff5024c7e0;docker-library/docker@d576eb69d7bad654b934176e95644995aa85d8f8:29/dind",
    dind_entrypoint_sha256: "acf43f8eb1181afbada127661c7d85ebbcf9e3b556d55c314991d2d21c25292a",
    runner_uid: 1_001,
    runner_gid: 1_001,
    runner_docker_gid: 123,
    dind_socket_group: "docker",
    dind_socket_gid: 2_375,
};

// The official image consumes JIT through its supported input environment. The
// payload arrives on Docker attach stdin, never in Docker Env, Cmd, or labels.
// A loaded host LSM profile remains a prerequisite before this path can run jobs.
fn official_runner_bootstrap(profile: &RunnerImageProfile) -> String {
    format!(
        concat!(
            "set -eu\n",
            "ready=0\n",
            "for attempt in $(seq 120); do\n",
            "  if docker ps >/dev/null 2>&1; then ready=1; break; fi\n",
            "  sleep 1\n",
            "done\n",
            "[ \"$ready\" -eq 1 ]\n",
            "sudo -n chown {uid}:{gid} /home/runner/_work /home/runner/externals\n",
            "IFS= read -r ACTIONS_RUNNER_INPUT_JITCONFIG || [ -n \"${{ACTIONS_RUNNER_INPUT_JITCONFIG:-}}\" ]\n",
            "[ -n \"${{ACTIONS_RUNNER_INPUT_JITCONFIG:-}}\" ]\n",
            "export ACTIONS_RUNNER_INPUT_JITCONFIG\n",
            "exec /home/runner/run.sh\n",
        ),
        uid = profile.runner_uid,
        gid = profile.runner_gid,
    )
}

/// Resolve the only currently reviewed official Linux image profile.
///
/// The public registry metadata audit found the current official runner image is
/// Ubuntu 24.04; it did not find an Ubuntu 26.04 image. This resolver deliberately
/// refuses to map the existing 26.04 selector to the 24.04 image. The runner is
/// requalified at least every 30 days from the upstream release publication time.
///
/// # Errors
///
/// Returns [`HostError::Config`] for an unknown key, selector mismatch, or stale image.
pub fn resolve_runner_profile(
    profile: &str,
    scale_set_name: &str,
) -> Result<RunnerImageProfile, HostError> {
    resolve_runner_profile_at(profile, scale_set_name, SystemTime::now())
}

pub(super) fn resolve_runner_profile_at(
    profile: &str,
    scale_set_name: &str,
    now: SystemTime,
) -> Result<RunnerImageProfile, HostError> {
    if profile != RUNNER_PROFILE.key || scale_set_name != RUNNER_PROFILE.scale_set_name {
        return Err(HostError::Config);
    }
    RUNNER_PROFILE.ensure_fresh(now)?;
    Ok(RUNNER_PROFILE)
}

/// Build a plan for the pinned official runner image and private `DinD` socket.
///
/// The JIT value remains outside Docker configuration. `worker::deliver_jit`
/// feeds it over attached stdin after the runner container starts. The selected
/// `AppArmor` profile is mandatory; Docker fails a create/start if it is absent.
///
/// # Errors
///
/// Returns [`HostError::Config`] when the image profile is stale and
/// [`HostError::ForbiddenMount`] for an invalid worker identity.
pub fn runner_plan_for_profile(
    private_volume: &str,
    profile: &RunnerImageProfile,
) -> Result<ContainerPlan, HostError> {
    profile.ensure_fresh(SystemTime::now())?;
    if *profile != RUNNER_PROFILE {
        return Err(HostError::Config);
    }
    if !private_volume_name(private_volume) {
        return Err(HostError::ForbiddenMount);
    }
    Ok(profiled_runner_plan(private_volume, profile))
}

fn profiled_runner_plan(private_volume: &str, profile: &RunnerImageProfile) -> ContainerPlan {
    let work = format!("{private_volume}-work");
    let externals = format!("{private_volume}-externals");
    ContainerPlan {
        name: format!("{private_volume}-runner"),
        privileged: false,
        platform: profile.platform.to_owned(),
        image: profile.runner_image.to_owned(),
        env: vec![
            "DOCKER_HOST=unix:///run/docker/docker.sock".to_owned(),
            "RUNNER_WAIT_FOR_DOCKER_IN_SECONDS=120".to_owned(),
        ],
        cmd: vec![
            "/bin/bash".to_owned(),
            "-c".to_owned(),
            official_runner_bootstrap(profile),
        ],
        labels: super::runner_labels(private_volume),
        mounts: vec![
            Mount {
                source: format!("volume:{private_volume}"),
                target: OFFICIAL_SOCKET_TARGET.to_owned(),
            },
            Mount {
                source: format!("volume:{work}"),
                target: runner_work_path(),
            },
            Mount {
                source: format!("volume:{externals}"),
                target: EXTERNALS_TARGET.to_owned(),
            },
        ],
        group_add: vec![profile.dind_socket_gid.to_string()],
        security_opts: vec![APPARMOR_RUNNER.to_owned()],
    }
}

pub(super) fn is_supported_runner_image(image: &str) -> bool {
    image == RUNNER_PROFILE.runner_image
}

pub(super) fn audit_official_plan(plan: &ContainerPlan) -> Result<(), HostError> {
    RUNNER_PROFILE.ensure_fresh(SystemTime::now())?;
    let volume = plan
        .name
        .strip_suffix("-runner")
        .filter(|volume| private_volume_name(volume))
        .ok_or(HostError::ForbiddenMount)?;
    let expected = profiled_runner_plan(volume, &RUNNER_PROFILE);
    if plan.env != expected.env
        || plan.cmd != expected.cmd
        || plan.labels != expected.labels
        || plan.mounts != expected.mounts
        || plan.group_add != expected.group_add
        || plan.security_opts != expected.security_opts
        || super::contains_payload_marker(
            &plan.env,
            &plan.cmd,
            &plan.labels,
            &plan.security_opts,
            &plan.mounts,
        )
    {
        return Err(HostError::ForbiddenMount);
    }
    Ok(())
}
