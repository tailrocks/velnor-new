//! Readonly qualified executable probes; the orchestrator hashes installed bytes.
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use velnor_actions_contract::{digest_b3, is_valid_digest};
use velnor_actions_contract_config::config::{
    CheckPlatform, MAX_CHECK_CONTAINER_PATH_BYTES, MAX_CHECK_QUALIFIED_PROBE_CAPTURE_BYTES,
    QualifiedTool, QualifiedToolExecutable, QualifiedToolProbe,
};
use velnor_actions_mise_core::{CheckDeadline, IsolatedCommand, MiseError};

/// Actual executable identity computed from installed bytes by the runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedExecutableObservation {
    /// Declared command name, never a caller-selected executable argument.
    pub name: String,
    /// Canonical executable path inside the owned installation closure.
    pub path: PathBuf,
    /// SHA-256 independently computed over those executable bytes.
    pub sha256: String,
}

/// Owned runtime homes and an explicitly selected compiler installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedProbeHomes {
    /// Canonical job-private root.
    pub home: PathBuf,
    /// Owned projections of qualified dependency executables.
    pub bin_dir: PathBuf,
    /// Owned Cargo cache and configuration home.
    pub cargo_home: PathBuf,
    /// Owned Rust distribution-management home.
    pub rust_home: PathBuf,
    /// Exact owned compiler prefix, never a downloadable version selector.
    pub compiler_toolchain: Option<PathBuf>,
}

/// Receipt of one fixed, bounded executable probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedExecutableProof {
    /// Root qualified-tool identity.
    pub tool_id: String,
    /// Exact declared tool release.
    pub tool_version: String,
    /// Explicit observed execution platform.
    pub platform: CheckPlatform,
    /// Complete declared executable and readonly probe.
    pub declared: QualifiedToolExecutable,
    /// Runtime byte identity checked before executing the probe.
    pub observed: QualifiedExecutableObservation,
    /// Complete bounded version stdout, retained for receipt validation.
    pub stdout: String,
    /// Digest of the exact stdout bytes.
    pub stdout_digest: String,
    /// Digest of the exact stderr bytes.
    pub stderr_digest: String,
}

/// Execute an exact declared version probe without consumer config or credentials.
///
/// The runtime must hash installed bytes itself before supplying `observed`.
/// No hashing subprocess, installation, shell, or configurable argv is used here.
/// # Errors
/// Rejects declaration/hash/path/version/platform drift or failed bounded probes.
pub fn verify_qualified_executable(
    tool: &QualifiedTool,
    platform: CheckPlatform,
    declared: &QualifiedToolExecutable,
    observed: &QualifiedExecutableObservation,
    homes: &QualifiedProbeHomes,
    deadline: CheckDeadline,
) -> Result<QualifiedExecutableProof, MiseError> {
    tool.validate("qualified_tools", &tool.id)
        .map_err(|e| invalid(e.to_string()))?;
    validate_membership(tool, platform, declared)?;
    validate_observation(declared, observed)?;
    let env = probe_environment(homes, tool.requires_compiler())?;
    if !observed.path.starts_with(&homes.home) {
        return Err(invalid("executable_outside_owned_installation"));
    }
    let canonical = observed
        .path
        .canonicalize()
        .map_err(|e| invalid(e.to_string()))?;
    if canonical != observed.path {
        return Err(invalid("executable_path_not_canonical"));
    }
    let args = match declared.probe {
        QualifiedToolProbe::Version { .. } | QualifiedToolProbe::CargoNextestVersion { .. } => {
            vec![OsString::from("--version")]
        }
        QualifiedToolProbe::VersionSubcommand { .. } => vec![OsString::from("version")],
        QualifiedToolProbe::RustcVerbose { .. } => vec![OsString::from("-vV")],
    };
    let command =
        IsolatedCommand::qualified_check_probe(observed.path.as_os_str().to_owned(), args, env);
    let output = command.run_until(MAX_CHECK_QUALIFIED_PROBE_CAPTURE_BYTES, deadline)?;
    if !output.success {
        return Err(invalid("version_probe_failed"));
    }
    let stdout_digest = digest_b3(&output.stdout);
    let stderr_digest = digest_b3(&output.stderr);
    let stdout =
        String::from_utf8(output.stdout).map_err(|_| invalid("version_stdout_not_utf8"))?;
    validate_output(&tool.version, platform, &declared.probe, &stdout)?;
    Ok(QualifiedExecutableProof {
        tool_id: tool.id.clone(),
        tool_version: tool.version.clone(),
        platform,
        declared: declared.clone(),
        observed: observed.clone(),
        stdout,
        stdout_digest,
        stderr_digest,
    })
}

/// Validate ordered executable receipts against their complete root declarations.
/// # Errors
/// Rejects missing, extra, reordered, altered, or semantically drifting receipts.
pub fn validate_executable_proofs(
    tool: &QualifiedTool,
    platform: CheckPlatform,
    proofs: &[QualifiedExecutableProof],
) -> Result<(), MiseError> {
    tool.validate("qualified_tools", &tool.id)
        .map_err(|e| invalid(e.to_string()))?;
    let qualification = tool
        .platforms
        .iter()
        .find(|p| p.platform == platform)
        .ok_or_else(|| invalid("platform_not_qualified"))?;
    if qualification.executables.len() != proofs.len() {
        return Err(invalid("executable_proof_count_mismatch"));
    }
    for (declared, proof) in qualification.executables.iter().zip(proofs) {
        if &proof.declared != declared
            || proof.tool_id != tool.id
            || proof.tool_version != tool.version
            || proof.platform != platform
            || proof.stdout_digest != digest_b3(proof.stdout.as_bytes())
            || !is_valid_digest(&proof.stderr_digest)
        {
            return Err(invalid("executable_proof_identity_mismatch"));
        }
        validate_observation(declared, &proof.observed)?;
        validate_output(&tool.version, platform, &declared.probe, &proof.stdout)?;
    }
    Ok(())
}

fn validate_membership(
    tool: &QualifiedTool,
    platform: CheckPlatform,
    declared: &QualifiedToolExecutable,
) -> Result<(), MiseError> {
    let qualification = tool
        .platforms
        .iter()
        .find(|p| p.platform == platform)
        .ok_or_else(|| invalid("platform_not_qualified"))?;
    if !qualification.executables.contains(declared) {
        return Err(invalid("executable_not_declared"));
    }
    if std::env::consts::OS != platform.os() || std::env::consts::ARCH != platform.arch() {
        return Err(invalid("probe_host_platform_mismatch"));
    }
    Ok(())
}

fn validate_observation(
    declared: &QualifiedToolExecutable,
    observed: &QualifiedExecutableObservation,
) -> Result<(), MiseError> {
    if observed.name != declared.name
        || observed.sha256 != declared.sha256
        || !observed.path.is_absolute()
        || !observed.path.to_str().is_some_and(|path| {
            path.len() <= MAX_CHECK_CONTAINER_PATH_BYTES && !path.chars().any(char::is_control)
        })
        || !velnor_actions_contract::ids::is_lower_hex_len(&observed.sha256, 64)
    {
        return Err(invalid("executable_digest_or_identity_mismatch"));
    }
    Ok(())
}

fn probe_environment(
    homes: &QualifiedProbeHomes,
    requires_compiler: bool,
) -> Result<Vec<(OsString, OsString)>, MiseError> {
    let compiler = if requires_compiler {
        Some(
            homes
                .compiler_toolchain
                .as_ref()
                .ok_or_else(|| invalid("owned_compiler_prefix_required"))?,
        )
    } else {
        None
    };
    if !homes.home.is_absolute()
        || homes
            .home
            .canonicalize()
            .map_err(|e| invalid(e.to_string()))?
            != homes.home
    {
        return Err(invalid("probe_home_not_canonical"));
    }
    for path in [&homes.bin_dir, &homes.cargo_home, &homes.rust_home]
        .into_iter()
        .chain(compiler)
    {
        if !path.starts_with(&homes.home)
            || path.canonicalize().map_err(|e| invalid(e.to_string()))? != *path
        {
            return Err(invalid("probe_path_not_owned_canonical"));
        }
    }
    let path = std::env::join_paths([
        homes.bin_dir.as_path(),
        Path::new("/usr/bin"),
        Path::new("/bin"),
        Path::new("/usr/sbin"),
        Path::new("/sbin"),
    ])
    .map_err(|e| invalid(e.to_string()))?;
    let mut env = vec![
        (OsString::from("HOME"), homes.home.as_os_str().to_owned()),
        (OsString::from("PATH"), path),
        (
            OsString::from("CARGO_HOME"),
            homes.cargo_home.as_os_str().to_owned(),
        ),
        (
            OsString::from("RUSTUP_HOME"),
            homes.rust_home.as_os_str().to_owned(),
        ),
        (OsString::from("LC_ALL"), OsString::from("C")),
        (OsString::from("MISE_AUTO_INSTALL"), OsString::from("false")),
        (
            OsString::from("MISE_EXEC_AUTO_INSTALL"),
            OsString::from("false"),
        ),
    ];
    if let Some(prefix) = compiler {
        env.push((
            OsString::from("RUSTUP_TOOLCHAIN"),
            prefix.as_os_str().to_owned(),
        ));
    }
    Ok(env)
}

fn validate_output(
    version: &str,
    platform: CheckPlatform,
    probe: &QualifiedToolProbe,
    stdout: &str,
) -> Result<(), MiseError> {
    if stdout.len() > MAX_CHECK_QUALIFIED_PROBE_CAPTURE_BYTES
        || stdout
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(invalid("version_stdout_exceeds_capture_limit"));
    }
    let expected = match probe {
        QualifiedToolProbe::Version { expected }
        | QualifiedToolProbe::VersionSubcommand { expected }
        | QualifiedToolProbe::CargoNextestVersion { expected }
        | QualifiedToolProbe::RustcVerbose { expected } => expected,
    };
    let actual = if matches!(probe, QualifiedToolProbe::RustcVerbose { .. }) {
        stdout.trim_end_matches(['\r', '\n'])
    } else {
        stdout
            .lines()
            .next()
            .ok_or_else(|| invalid("empty_version_stdout"))?
    };
    if actual != expected.trim_end_matches(['\r', '\n'])
        || actual.len() > MAX_CHECK_QUALIFIED_PROBE_CAPTURE_BYTES
    {
        return Err(invalid("exact_version_output_mismatch"));
    }
    if matches!(probe, QualifiedToolProbe::RustcVerbose { .. }) {
        validate_rustc(version, platform, actual)
    } else if version_token(actual, version) {
        Ok(())
    } else {
        Err(invalid("declared_version_token_missing"))
    }
}

fn validate_rustc(version: &str, platform: CheckPlatform, output: &str) -> Result<(), MiseError> {
    for (key, expected) in [("release: ", version), ("host: ", platform.target())] {
        let values: Vec<&str> = output
            .lines()
            .filter_map(|line| line.strip_prefix(key))
            .collect();
        if values != [expected] {
            return Err(invalid("rustc_release_or_host_mismatch"));
        }
    }
    let hashes: Vec<&str> = output
        .lines()
        .filter_map(|line| line.strip_prefix("commit-hash: "))
        .collect();
    if hashes.len() != 1 || !velnor_actions_contract::ids::is_lower_hex_len(hashes[0], 40) {
        return Err(invalid("rustc_commit_identity_missing"));
    }
    Ok(())
}

fn version_token(output: &str, version: &str) -> bool {
    output.match_indices(version).any(|(start, _)| {
        let end = start + version.len();
        let boundary =
            |b: u8| !b.is_ascii_alphanumeric() && !matches!(b, b'.' | b'_' | b'-' | b'+');
        (start == 0 || boundary(output.as_bytes()[start - 1]))
            && (end == output.len() || boundary(output.as_bytes()[end]))
    })
}

fn invalid(value: impl Into<String>) -> MiseError {
    MiseError::InvalidStepInput {
        field: "qualified_tool_probe".into(),
        value: value.into(),
    }
}
