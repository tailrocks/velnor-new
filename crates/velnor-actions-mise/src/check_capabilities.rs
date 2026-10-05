//! Readonly container observations over orchestrator-verified owned binaries.
use super::invalid;
use crate::{CheckDeadline, IsolatedCommand, MiseError};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use velnor_actions_contract::config::{
    CheckRunner, HostContainerProfile, MAX_CHECK_CONTAINER_DAEMON_CAPTURE_BYTES,
    MAX_CHECK_CONTAINER_PATH_BYTES, MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES,
};
#[path = "check_container_observation.rs"]
mod observation;
pub use observation::{
    ContainerObservation, ContainerProbeOutput, DockerDaemonObservation, OrbStackAppObservation,
    OrbStackObservation, validate_check_capability_proof,
};
#[path = "check_capabilities_probe.rs"]
mod probe;
use probe::{probe_failure, probe_orbstack};

/// Owned bytes and runtime paths verified and prepared by the orchestrator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedContainer {
    /// Job-private canonical home containing all projected executables.
    pub home: PathBuf,
    /// Private credential-free Docker metadata/config directory.
    pub docker_config: PathBuf,
    /// Immutable owned Docker executable.
    pub docker_program: PathBuf,
    /// Observed SHA-256 of those copied Docker bytes.
    pub docker_sha256: String,
    /// Immutable owned nested `OrbStack` CLI executable, when requested.
    pub orbctl_program: Option<PathBuf>,
    /// Observed SHA-256 of the owned `OrbStack` CLI bytes.
    pub orbctl_sha256: Option<String>,
    /// Explicit local endpoint whose socket type the orchestrator verified.
    pub endpoint: String,
}

/// Required container observations, absent only for a profile without a container.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckCapabilityProof {
    /// Complete declaration-bound observed container identity.
    pub container: Option<ContainerObservation>,
}

/// Probe prepared binaries, their declared context and the actual daemon.
/// No ambient executable, home, context or credential lookup is performed.
/// # Errors
/// Rejects missing preparation, undeclared programs, version/hash/daemon drift.
pub fn verify_check_capabilities(
    runner: &CheckRunner,
    prepared: Option<&PreparedContainer>,
    deadline: CheckDeadline,
) -> Result<CheckCapabilityProof, MiseError> {
    let Some(profile) = &runner.container else {
        if prepared.is_some() {
            return Err(invalid("container", "unexpected_preparation"));
        }
        return Ok(CheckCapabilityProof { container: None });
    };
    let prepared = prepared.ok_or_else(|| invalid("container", "required_preparation_missing"))?;
    profile
        .validate(
            runner.platform,
            runner.executor,
            "checks",
            "runner.container",
        )
        .map_err(|error| invalid("container", error.to_string()))?;
    validate_prepared(profile, prepared)?;
    let env = capability_environment(prepared);
    let docker_cli = probe(
        &prepared.docker_program,
        &["--version"],
        &env,
        deadline,
        MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES,
    )?;
    let context = match profile {
        HostContainerProfile::Docker { context, .. }
        | HostContainerProfile::OrbStack { context, .. } => context,
    };
    let inspected = probe(
        &prepared.docker_program,
        &[
            "context",
            "inspect",
            context,
            "--format",
            "{{.Endpoints.docker.Host}}",
        ],
        &env,
        deadline,
        MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES,
    )?;
    if inspected.stdout.trim_end_matches(['\r', '\n']) != prepared.endpoint {
        return Err(invalid("container_context", "prepared_endpoint_mismatch"));
    }
    let daemon = probe(
        &prepared.docker_program,
        &[
            "--host",
            &prepared.endpoint,
            "info",
            "--format",
            observation::DAEMON_FORMAT,
        ],
        &env,
        deadline,
        MAX_CHECK_CONTAINER_DAEMON_CAPTURE_BYTES,
    )?;
    let daemon = DockerDaemonObservation::parse(daemon)?;
    let orbctl = match profile {
        HostContainerProfile::OrbStack { sdk, .. } => {
            Some(probe_orbstack(sdk, prepared, &env, deadline)?)
        }
        HostContainerProfile::Docker { .. } => None,
    };
    let proof = CheckCapabilityProof {
        container: Some(ContainerObservation {
            profile: profile.clone(),
            docker_program: prepared.docker_program.clone(),
            docker_sha256: prepared.docker_sha256.clone(),
            endpoint: prepared.endpoint.clone(),
            docker_cli,
            context_probe: inspected,
            daemon,
            orbctl,
        }),
    };
    validate_check_capability_proof(runner, &proof)?;
    Ok(proof)
}

fn validate_prepared(
    profile: &HostContainerProfile,
    prepared: &PreparedContainer,
) -> Result<(), MiseError> {
    if !prepared.home.is_absolute()
        || !bounded_path(&prepared.home)
        || prepared
            .home
            .canonicalize()
            .map_err(|e| invalid("container_home", e.to_string()))?
            != prepared.home
    {
        return Err(invalid("container_home", "canonical_owned_home_required"));
    }
    for path in [&prepared.docker_config, &prepared.docker_program]
        .into_iter()
        .chain(prepared.orbctl_program.as_ref())
    {
        if !bounded_path(path)
            || !path.starts_with(&prepared.home)
            || path
                .canonicalize()
                .map_err(|e| invalid("container_path", e.to_string()))?
                != *path
        {
            return Err(invalid("container_path", "canonical_owned_path_required"));
        }
    }
    let cli = match profile {
        HostContainerProfile::Docker { cli, .. } | HostContainerProfile::OrbStack { cli, .. } => {
            cli
        }
    };
    if prepared.docker_sha256 != cli.sha256 {
        return Err(invalid("docker_cli", "prepared_hash_mismatch"));
    }
    match profile {
        HostContainerProfile::Docker { .. }
            if prepared.orbctl_program.is_some() || prepared.orbctl_sha256.is_some() =>
        {
            return Err(invalid("orbstack", "undeclared_cli"));
        }
        HostContainerProfile::OrbStack { sdk, .. }
            if prepared.orbctl_program.is_none()
                || prepared.orbctl_sha256.as_ref() != Some(&sdk.cli_sha256) =>
        {
            return Err(invalid("orbstack", "required_verified_cli_missing"));
        }
        _ => {}
    }
    let socket = prepared
        .endpoint
        .strip_prefix("unix://")
        .ok_or_else(|| invalid("container_endpoint", "local_unix_socket_required"))?;
    if prepared.endpoint != format!("unix://{}", profile.socket_path())
        || !Path::new(socket).is_absolute()
        || socket.len() > MAX_CHECK_CONTAINER_PATH_BYTES
    {
        return Err(invalid("container_endpoint", "absolute_socket_required"));
    }
    Ok(())
}

fn bounded_path(path: &Path) -> bool {
    path.to_str().is_some_and(|value| {
        value.len() <= MAX_CHECK_CONTAINER_PATH_BYTES && !value.chars().any(char::is_control)
    })
}

fn capability_environment(prepared: &PreparedContainer) -> Vec<(OsString, OsString)> {
    [
        ("HOME", prepared.home.as_os_str().to_owned()),
        (
            "DOCKER_CONFIG",
            prepared.docker_config.as_os_str().to_owned(),
        ),
        ("DOCKER_HOST", OsString::from(&prepared.endpoint)),
        ("PATH", OsString::from("/usr/bin:/bin:/usr/sbin:/sbin")),
        ("LC_ALL", OsString::from("C")),
    ]
    .into_iter()
    .map(|(key, value)| (OsString::from(key), value))
    .collect()
}

fn probe(
    program: &Path,
    args: &[&str],
    env: &[(OsString, OsString)],
    deadline: CheckDeadline,
    capture_limit: usize,
) -> Result<ContainerProbeOutput, MiseError> {
    let command = IsolatedCommand::qualified_check_probe(
        program.as_os_str().to_owned(),
        args.iter().map(OsString::from).collect(),
        env.to_vec(),
    );
    let result = command.run_until(capture_limit, deadline)?;
    if !result.success {
        return Err(probe_failure(program, args, &result));
    }
    let stdout_digest = velnor_actions_contract::digest_b3(&result.stdout);
    let stderr_digest = velnor_actions_contract::digest_b3(&result.stderr);
    let stdout =
        String::from_utf8(result.stdout).map_err(|_| invalid("container_probe", "invalid_utf8"))?;
    let stderr = String::from_utf8(result.stderr)
        .map_err(|_| invalid("container_probe", "invalid_stderr_utf8"))?;
    Ok(ContainerProbeOutput {
        stdout,
        stderr,
        stdout_digest,
        stderr_digest,
    })
}

#[cfg(all(test, unix))]
#[path = "check_capabilities_encoding_tests.rs"]
mod encoding_tests;
