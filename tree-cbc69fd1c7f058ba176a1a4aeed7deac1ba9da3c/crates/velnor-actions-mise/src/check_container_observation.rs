use super::CheckCapabilityProof;
use crate::MiseError;
use crate::checks::invalid;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use velnor_actions_contract::config::{
    CheckRunner, ContainerPlatform, HostContainerProfile, MAX_CHECK_CONTAINER_DAEMON_CAPTURE_BYTES,
    MAX_CHECK_CONTAINER_PATH_BYTES, MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES,
};
use velnor_actions_contract::{digest_b3, is_valid_digest};
#[path = "check_orbstack_app_observation.rs"]
mod app;
pub use app::OrbStackAppObservation;
#[cfg(test)]
use velnor_actions_contract::config::MAX_CHECK_CONTAINER_APP_INFO_CAPTURE_BYTES;

pub(super) const DAEMON_FORMAT: &str = r#"{"ID":{{json .ID}},"ServerVersion":{{json .ServerVersion}},"OSType":{{json .OSType}},"Architecture":{{json .Architecture}},"OperatingSystem":{{json .OperatingSystem}}}"#;

/// Exact bounded successful probe output and stream identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerProbeOutput {
    /// Exact UTF-8 stdout bytes.
    pub stdout: String,
    /// Exact UTF-8 stderr bytes (codesign identity is reported here).
    pub stderr: String,
    /// BLAKE3 identity of stdout bytes.
    pub stdout_digest: String,
    /// BLAKE3 identity of stderr bytes.
    pub stderr_digest: String,
}

/// Stable daemon identity from explicit Docker JSON fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DockerDaemonObservation {
    /// Actual daemon identity, distinct from its host or Docker CLI identity.
    pub id: String,
    /// Actual daemon release.
    pub version: String,
    /// Actual daemon execution platform.
    pub platform: ContainerPlatform,
    /// Unnormalized architecture reported by Docker.
    pub architecture: String,
    /// Actual daemon operating system identity.
    pub operating_system: String,
    /// Exact selected JSON fields and stream identities.
    pub probe: ContainerProbeOutput,
}

/// Version and running-state observations from the prepared nested `OrbStack` CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrbStackObservation {
    /// Owned canonical nested CLI executable.
    pub program: PathBuf,
    /// Verified nested CLI executable SHA-256.
    pub sha256: String,
    /// CLI version/build/commit output, distinct from application bundle version.
    pub version: ContainerProbeOutput,
    /// Actual application identity and successful integrity observations.
    pub app: OrbStackAppObservation,
    /// Exact running-state output.
    pub status: ContainerProbeOutput,
}

/// Complete root declaration and stable container observations for replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerObservation {
    /// Complete root-qualified profile.
    pub profile: HostContainerProfile,
    /// Owned canonical Docker executable.
    pub docker_program: PathBuf,
    /// Verified Docker executable SHA-256.
    pub docker_sha256: String,
    /// Actual explicit local daemon socket endpoint.
    pub endpoint: String,
    /// Exact Docker CLI version/build output.
    pub docker_cli: ContainerProbeOutput,
    /// Context observation proving endpoint selection.
    pub context_probe: ContainerProbeOutput,
    /// Stable server identity independent from host architecture.
    pub daemon: DockerDaemonObservation,
    /// Required nested CLI observations for `OrbStack` profiles.
    pub orbctl: Option<OrbStackObservation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DockerJson {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "ServerVersion")]
    version: String,
    #[serde(rename = "OSType")]
    os: String,
    #[serde(rename = "Architecture")]
    architecture: String,
    #[serde(rename = "OperatingSystem")]
    operating_system: String,
}
impl DockerDaemonObservation {
    pub(super) fn parse(probe: ContainerProbeOutput) -> Result<Self, MiseError> {
        validate_streams(&probe, MAX_CHECK_CONTAINER_DAEMON_CAPTURE_BYTES)?;
        let json: DockerJson = serde_json::from_str(&probe.stdout)
            .map_err(|_| invalid("container_daemon", "invalid_fixed_json"))?;
        let platform = match (json.os.as_str(), json.architecture.as_str()) {
            ("linux", "x86_64" | "amd64") => ContainerPlatform::LinuxX64,
            ("linux", "aarch64" | "arm64") => ContainerPlatform::LinuxArm64,
            _ => return Err(invalid("container_daemon", "unsupported_daemon_platform")),
        };
        if json.id.is_empty()
            || json.id.len() > 256
            || !json.id.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(invalid("container_daemon", "missing_stable_daemon_id"));
        }
        Ok(Self {
            id: json.id,
            version: json.version,
            platform,
            architecture: json.architecture,
            operating_system: json.operating_system,
            probe,
        })
    }
}

/// Validate mandatory declaration-bound observations for final receipt replay.
/// # Errors
/// Rejects missing/extra profiles, stream tampering, version or daemon drift.
pub fn validate_check_capability_proof(
    runner: &CheckRunner,
    proof: &CheckCapabilityProof,
) -> Result<(), MiseError> {
    if let Some(profile) = &runner.container {
        profile
            .validate(
                runner.platform,
                runner.executor,
                "checks",
                "runner.container",
            )
            .map_err(|error| invalid("container", error.to_string()))?;
    }
    match (&runner.container, &proof.container) {
        (None, None) => Ok(()),
        (Some(profile), Some(observed)) if profile == &observed.profile => {
            validate_observation(observed)
        }
        _ => Err(invalid(
            "container_proof",
            "required_profile_identity_mismatch",
        )),
    }
}

fn validate_observation(observed: &ContainerObservation) -> Result<(), MiseError> {
    let (cli, daemon) = match &observed.profile {
        HostContainerProfile::Docker { cli, daemon, .. }
        | HostContainerProfile::OrbStack { cli, daemon, .. } => (cli, daemon),
    };
    let socket = observed
        .endpoint
        .strip_prefix("unix://")
        .ok_or_else(|| invalid("container_endpoint", "local_socket_required"))?;
    if observed.endpoint != format!("unix://{}", observed.profile.socket_path())
        || !Path::new(socket).is_absolute()
        || !bounded_path(&observed.docker_program)
        || socket.len() > MAX_CHECK_CONTAINER_PATH_BYTES
        || !observed.docker_program.is_absolute()
        || observed.docker_sha256 != cli.sha256
    {
        return Err(invalid("container_proof", "binary_or_endpoint_mismatch"));
    }
    validate_streams(
        &observed.docker_cli,
        MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES,
    )?;
    validate_streams(
        &observed.context_probe,
        MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES,
    )?;
    if exact(&observed.docker_cli.stdout)
        != format!("Docker version {}, build {}", cli.version, cli.build)
        || exact(&observed.context_probe.stdout) != observed.endpoint
    {
        return Err(invalid(
            "container_cli",
            "exact_version_or_context_mismatch",
        ));
    }
    if DockerDaemonObservation::parse(observed.daemon.probe.clone())? != observed.daemon
        || observed.daemon.version != daemon.version
        || observed.daemon.platform != daemon.platform
        || observed.daemon.operating_system != daemon.operating_system
    {
        return Err(invalid("container_daemon", "declared_daemon_mismatch"));
    }
    match (&observed.profile, &observed.orbctl) {
        (HostContainerProfile::Docker { .. }, None) => Ok(()),
        (HostContainerProfile::OrbStack { sdk, .. }, Some(orb)) => {
            if !bounded_path(&orb.program) {
                return Err(invalid("orbstack", "observation_path_limit"));
            }
            validate_streams(&orb.version, MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES)?;
            validate_streams(&orb.status, MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES)?;
            orb.app.validate(sdk)?;
            let expected = format!(
                "Version: {} ({})\nCommit: {} (v{})",
                sdk.cli_version, sdk.cli_build, sdk.cli_commit, sdk.cli_version
            );
            if !orb.program.is_absolute()
                || orb.sha256 != sdk.cli_sha256
                || exact(&orb.version.stdout) != expected
                || !orbstack_running(&orb.status.stdout)
                || observed.daemon.operating_system != "OrbStack"
            {
                return Err(invalid(
                    "orbstack",
                    "declared_cli_or_running_state_mismatch",
                ));
            }
            Ok(())
        }
        _ => Err(invalid("orbstack", "required_observation_mismatch")),
    }
}

fn validate_streams(probe: &ContainerProbeOutput, cap: usize) -> Result<(), MiseError> {
    if probe.stdout.len() > cap
        || probe.stderr.len() > cap
        || digest_b3(probe.stdout.as_bytes()) != probe.stdout_digest
        || digest_b3(probe.stderr.as_bytes()) != probe.stderr_digest
        || !is_valid_digest(&probe.stderr_digest)
        || !printable_probe_text(&probe.stdout)
        || !printable_probe_text(&probe.stderr)
    {
        return Err(invalid(
            "container_probe",
            "bounded_stream_identity_mismatch",
        ));
    }
    Ok(())
}

fn printable_probe_text(value: &str) -> bool {
    !value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
}

fn bounded_path(path: &Path) -> bool {
    path.to_str().is_some_and(|value| {
        value.len() <= MAX_CHECK_CONTAINER_PATH_BYTES && !value.chars().any(char::is_control)
    })
}
fn exact(stdout: &str) -> &str {
    stdout.trim_end_matches(['\r', '\n'])
}

fn orbstack_running(stdout: &str) -> bool {
    exact(stdout) == "Running"
}

#[cfg(test)]
#[path = "check_container_observation_tests.rs"]
mod tests;
