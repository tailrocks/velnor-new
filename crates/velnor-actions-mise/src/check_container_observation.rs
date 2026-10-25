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

pub(super) const DAEMON_FORMAT: &str = r#"{"ID":{{json .ID}},"ServerVersion":{{json .ServerVersion}},"OSType":{{json .OSType}},"Architecture":{{json .Architecture}},"OperatingSystem":{{json .OperatingSystem}}}"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerProbeOutput {
    pub stdout: String,
    pub stderr: String,
    pub stdout_digest: String,
    pub stderr_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DockerDaemonObservation {
    pub id: String,
    pub version: String,
    pub platform: ContainerPlatform,
    pub architecture: String,
    pub operating_system: String,
    pub probe: ContainerProbeOutput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrbStackObservation {
    pub program: PathBuf,
    pub sha256: String,
    pub version: ContainerProbeOutput,
    pub app: OrbStackAppObservation,
    pub status: ContainerProbeOutput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerObservation {
    pub profile: HostContainerProfile,
    pub docker_program: PathBuf,
    pub docker_sha256: String,
    pub endpoint: String,
    pub docker_cli: ContainerProbeOutput,
    pub context_probe: ContainerProbeOutput,
    pub daemon: DockerDaemonObservation,
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
