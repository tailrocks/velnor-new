use super::CheckCapabilityProof;
use crate::MiseError;
use crate::checks::invalid;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use velnor_actions_contract::config::{CheckRunner, ContainerPlatform, HostContainerProfile};
use velnor_actions_contract::{digest_b3, is_valid_digest};

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
        validate_streams(&probe)?;
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
        || !observed.docker_program.is_absolute()
        || observed.docker_sha256 != cli.sha256
    {
        return Err(invalid("container_proof", "binary_or_endpoint_mismatch"));
    }
    validate_streams(&observed.docker_cli)?;
    validate_streams(&observed.context_probe)?;
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
            validate_streams(&orb.version)?;
            validate_streams(&orb.status)?;
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

fn validate_streams(probe: &ContainerProbeOutput) -> Result<(), MiseError> {
    if probe.stdout.len() > 64 * 1024
        || probe.stderr.len() > 64 * 1024
        || digest_b3(probe.stdout.as_bytes()) != probe.stdout_digest
        || digest_b3(probe.stderr.as_bytes()) != probe.stderr_digest
        || !is_valid_digest(&probe.stderr_digest)
    {
        return Err(invalid(
            "container_probe",
            "bounded_stream_identity_mismatch",
        ));
    }
    Ok(())
}
fn exact(stdout: &str) -> &str {
    stdout.trim_end_matches(['\r', '\n'])
}

fn orbstack_running(stdout: &str) -> bool {
    exact(stdout) == "Running"
}

/// Actual signed application identity, separate from CLI reported version/build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrbStackAppObservation {
    /// Actual Info.plist bundle identifier.
    pub bundle_id: String,
    /// Actual application marketing release.
    pub version: String,
    /// Actual application bundle build.
    pub build: String,
    /// Actual signed developer team identity.
    pub team_id: String,
    /// Exact Info.plist JSON observation.
    pub info: ContainerProbeOutput,
    /// Exact codesign identity observation.
    pub signature: ContainerProbeOutput,
    /// Successful strict outer application verification.
    pub outer_integrity: ContainerProbeOutput,
    /// Successful strict source nested CLI verification.
    pub source_cli_integrity: ContainerProbeOutput,
    /// Successful strict owned nested CLI verification.
    pub owned_cli_integrity: ContainerProbeOutput,
    /// Actual source nested CLI signed identity.
    pub source_cli_signature: ContainerProbeOutput,
    /// Actual owned nested CLI signed identity.
    pub owned_cli_signature: ContainerProbeOutput,
}
impl OrbStackAppObservation {
    pub(super) fn parse(
        info: ContainerProbeOutput,
        signature: ContainerProbeOutput,
        outer_integrity: ContainerProbeOutput,
        source_cli_integrity: ContainerProbeOutput,
        owned_cli_integrity: ContainerProbeOutput,
        source_cli_signature: ContainerProbeOutput,
        owned_cli_signature: ContainerProbeOutput,
    ) -> Result<Self, MiseError> {
        for output in [
            &info,
            &signature,
            &outer_integrity,
            &source_cli_integrity,
            &owned_cli_integrity,
            &source_cli_signature,
            &owned_cli_signature,
        ] {
            validate_streams(output)?;
        }
        let json: serde_json::Value = serde_json::from_str(&info.stdout)
            .map_err(|_| invalid("orbstack_app", "invalid_plist_json"))?;
        let value = |key: &str| {
            json.get(key)
                .and_then(serde_json::Value::as_str)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| invalid("orbstack_app", "missing_identity_field"))
        };
        let bundle_id = value("CFBundleIdentifier")?;
        let version = value("CFBundleShortVersionString")?;
        let build = value("CFBundleVersion")?;
        let teams: Vec<_> = signature
            .stderr
            .lines()
            .filter_map(|line| line.strip_prefix("TeamIdentifier="))
            .collect();
        let ids: Vec<_> = signature
            .stderr
            .lines()
            .filter_map(|line| line.strip_prefix("Identifier="))
            .collect();
        if teams.len() != 1 || teams[0].is_empty() || ids != [bundle_id.as_str()] {
            return Err(invalid("orbstack_app", "missing_signed_identity"));
        }
        let team_id = teams[0].to_owned();
        let source_identity = signed_identity(&source_cli_signature)?;
        let owned_identity = signed_identity(&owned_cli_signature)?;
        if source_identity != owned_identity || source_identity.0 != team_id {
            return Err(invalid("orbstack_app", "nested_signed_identity_mismatch"));
        }
        Ok(Self {
            bundle_id,
            version,
            build,
            team_id,
            info,
            signature,
            outer_integrity,
            source_cli_integrity,
            owned_cli_integrity,
            source_cli_signature,
            owned_cli_signature,
        })
    }
    fn validate(
        &self,
        sdk: &velnor_actions_contract::config::HostOrbStackSdk,
    ) -> Result<(), MiseError> {
        validate_main_executable(&self.info.stdout, &sdk.main_executable_path)?;
        let parsed = Self::parse(
            self.info.clone(),
            self.signature.clone(),
            self.outer_integrity.clone(),
            self.source_cli_integrity.clone(),
            self.owned_cli_integrity.clone(),
            self.source_cli_signature.clone(),
            self.owned_cli_signature.clone(),
        )?;
        if &parsed != self
            || self.bundle_id != sdk.bundle_id
            || self.version != sdk.version
            || self.build != sdk.build
            || self.team_id != sdk.team_id
        {
            return Err(invalid("orbstack_app", "declared_signed_app_mismatch"));
        }
        Ok(())
    }
}

fn validate_main_executable(info: &str, declared: &str) -> Result<(), MiseError> {
    let json: serde_json::Value =
        serde_json::from_str(info).map_err(|_| invalid("orbstack_app", "invalid_plist_json"))?;
    let name = json
        .get("CFBundleExecutable")
        .and_then(serde_json::Value::as_str)
        .filter(|name| {
            !name.is_empty() && !name.contains(['/', '\\']) && !matches!(*name, "." | "..")
        })
        .ok_or_else(|| invalid("orbstack_app", "invalid_main_executable_name"))?;
    if declared != format!("Contents/MacOS/{name}") {
        return Err(invalid("orbstack_app", "declared_main_executable_mismatch"));
    }
    Ok(())
}

fn signed_identity(probe: &ContainerProbeOutput) -> Result<(&str, &str), MiseError> {
    let values = |prefix| {
        probe
            .stderr
            .lines()
            .filter_map(move |line| line.strip_prefix(prefix))
            .collect::<Vec<_>>()
    };
    let teams = values("TeamIdentifier=");
    let ids = values("Identifier=");
    if teams.len() != 1 || ids.len() != 1 || teams[0].is_empty() || ids[0].is_empty() {
        return Err(invalid("orbstack_app", "missing_nested_signed_identity"));
    }
    Ok((teams[0], ids[0]))
}

#[cfg(test)]
#[path = "check_container_observation_tests.rs"]
mod tests;
