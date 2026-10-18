//! Closed native tool probes: observation only, never installation.
use super::invalid;
use crate::{CheckDeadline, IsolatedCommand, MiseError, ProcessOutput};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::Path;
use velnor_actions_contract::canonical::{digest_b3, is_valid_digest};
use velnor_actions_contract::config::{CheckPlatform, CheckSystemTool, CheckSystemToolKind};

/// Native tool observation bound to its exact declared pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemToolProof {
    /// Exact repository declaration.
    pub declared: CheckSystemTool,
    /// Parsed product version.
    pub observed_version: String,
    /// Parsed complete compiler or product build identity.
    pub observed_build: String,
    /// Exact macOS SDK version selected from the developer directory; absent for Swift.
    pub observed_sdk_version: Option<String>,
    /// Digest of the exact SDK version stdout; absent for Swift.
    pub sdk_stdout_digest: Option<String>,
    /// Digest of the exact SDK version stderr; absent for Swift.
    pub sdk_stderr_digest: Option<String>,
    /// Complete native Swift target triple; absent for Xcode.
    pub observed_target: Option<String>,
    /// Canonical absolute executable identity.
    pub executable: String,
    /// Fixed system launcher used for observation.
    pub launcher: String,
    /// Canonical selected developer directory, bound into every later probe and task.
    pub developer_dir: String,
    /// Exact fixed system selector stdout digest.
    pub developer_stdout_digest: String,
    /// Exact fixed system selector stderr digest.
    pub developer_stderr_digest: String,
    /// Digest of the exact version stdout bytes.
    pub stdout_digest: String,
    /// Digest of the exact version stderr bytes.
    pub stderr_digest: String,
    /// Swift executable discovery stdout digest.
    pub discovery_stdout_digest: Option<String>,
    /// Swift executable discovery stderr digest.
    pub discovery_stderr_digest: Option<String>,
}

/// Observe native system tools using fixed system executables and arguments.
/// # Errors
/// Rejects unsupported hosts, missing tools, malformed output and pin drift.
pub fn verify_check_system_tools(
    platform: CheckPlatform,
    pins: &[CheckSystemTool],
    deadline: CheckDeadline,
) -> Result<Vec<SystemToolProof>, MiseError> {
    if pins.is_empty() {
        return Ok(Vec::new());
    }
    if platform.os() != "macos"
        || std::env::consts::OS != platform.os()
        || std::env::consts::ARCH != platform.arch()
    {
        return Err(invalid("system_tools", "native_host_platform_mismatch"));
    }
    let selection = probe("/usr/bin/xcode-select", &["-p"], None, deadline)?;
    let selected = strict_text(&selection.stdout)?;
    if selected.lines().count() != 1 || !absolute_identity(selected.trim()) {
        return Err(invalid("system_tools", "invalid_developer_selection"));
    }
    let requested = pins
        .iter()
        .find_map(|pin| pin.developer_dir.as_deref())
        .unwrap_or(selected.trim());
    if pins.iter().any(|pin| {
        pin.developer_dir
            .as_deref()
            .is_some_and(|directory| directory != requested)
    }) {
        return Err(invalid("system_tools", "conflicting_developer_directories"));
    }
    let directory = Path::new(requested)
        .canonicalize()
        .map_err(|e| invalid("system_tools", e.to_string()))?;
    if !directory.is_dir() {
        return Err(invalid("system_tools", "missing_developer_directory"));
    }
    let directory = directory
        .to_str()
        .ok_or_else(|| invalid("system_tools", "invalid_developer_utf8"))?;
    if pins.iter().any(|pin| {
        pin.developer_dir
            .as_deref()
            .is_some_and(|configured| configured != directory)
    }) {
        return Err(invalid("system_tools", "developer_directory_not_canonical"));
    }
    let proofs = pins
        .iter()
        .map(|pin| observe(platform, pin, directory, &selection, deadline))
        .collect::<Result<Vec<_>, _>>()?;
    validate_system_tool_proofs(platform, pins, &proofs)?;
    Ok(proofs)
}

/// Check receipt observations against the repository declarations.
/// # Errors
/// Rejects missing, reordered, extra, malformed or drifting proofs.
pub fn validate_system_tool_proofs(
    platform: CheckPlatform,
    pins: &[CheckSystemTool],
    proofs: &[SystemToolProof],
) -> Result<(), MiseError> {
    if pins.len() != proofs.len() {
        return Err(invalid("system_tools", "proof_count_mismatch"));
    }
    for (pin, proof) in pins.iter().zip(proofs) {
        let swift = pin.kind == CheckSystemToolKind::Swift;
        let launcher = if swift {
            "/usr/bin/xcrun"
        } else {
            "/usr/bin/xcodebuild"
        };
        if &proof.declared != pin
            || proof.observed_version != pin.version
            || proof.observed_build != pin.build
            || proof.observed_sdk_version != pin.sdk_version
            || !valid_version(&pin.version)
            || !valid_build(pin.kind, &pin.build)
            || platform.os() != "macos"
            || !valid_proof_target(platform, proof.observed_target.as_deref(), swift)
            || proof.launcher != launcher
            || !absolute_identity(&proof.executable)
            || !absolute_identity(&proof.developer_dir)
            || pin
                .developer_dir
                .as_deref()
                .is_some_and(|expected| expected != proof.developer_dir)
            || proofs
                .first()
                .is_some_and(|first| first.developer_dir != proof.developer_dir)
            || (swift && !Path::new(&proof.executable).starts_with(&proof.developer_dir))
            || (!swift && proof.executable != "/usr/bin/xcodebuild")
            || (!swift
                && (proof
                    .observed_sdk_version
                    .as_deref()
                    .is_none_or(|v| !valid_version(v))
                    || pin.sdk_version.as_deref() != proof.observed_sdk_version.as_deref()
                    || proof
                        .sdk_stdout_digest
                        .as_deref()
                        .is_none_or(|v| !is_valid_digest(v))
                    || proof
                        .sdk_stderr_digest
                        .as_deref()
                        .is_none_or(|v| !is_valid_digest(v))))
            || (swift && (proof.sdk_stdout_digest.is_some() || proof.sdk_stderr_digest.is_some()))
            || !is_valid_digest(&proof.developer_stdout_digest)
            || !is_valid_digest(&proof.developer_stderr_digest)
            || !is_valid_digest(&proof.stdout_digest)
            || !is_valid_digest(&proof.stderr_digest)
            || proof.discovery_stdout_digest.is_some() != swift
            || proof.discovery_stderr_digest.is_some() != swift
            || proof
                .discovery_stdout_digest
                .as_deref()
                .is_some_and(|v| !is_valid_digest(v))
            || proof
                .discovery_stderr_digest
                .as_deref()
                .is_some_and(|v| !is_valid_digest(v))
        {
            return Err(invalid("system_tools", "proof_pin_or_identity_mismatch"));
        }
    }
    Ok(())
}

fn observe(
    platform: CheckPlatform,
    pin: &CheckSystemTool,
    directory: &str,
    selection: &ProcessOutput,
    deadline: CheckDeadline,
) -> Result<SystemToolProof, MiseError> {
    let swift = pin.kind == CheckSystemToolKind::Swift;
    let launcher = if swift {
        "/usr/bin/xcrun"
    } else {
        "/usr/bin/xcodebuild"
    };
    let discovery = if swift {
        Some(probe(
            launcher,
            &["--find", "swift"],
            Some(directory),
            deadline,
        )?)
    } else {
        None
    };
    let executable = match &discovery {
        Some(output) => {
            let text = strict_text(&output.stdout)?;
            if text.lines().count() != 1 || !absolute_identity(text.trim()) {
                return Err(invalid("system_tools", "invalid_swift_executable"));
            }
            canonical_executable(text.trim())?
        }
        None => canonical_executable(launcher)?,
    };
    let args: &[&str] = if swift {
        &["swift", "--version"]
    } else {
        &["-version"]
    };
    let output = probe(launcher, args, Some(directory), deadline)?;
    let (version, build) = parse_system_tool_version(pin.kind, &output.stdout, &output.stderr)?;
    let sdk_output = if swift {
        None
    } else {
        Some(observe_xcode_sdk(pin, directory, deadline)?)
    };
    let sdk_version = sdk_output.as_ref().map(|(version, _)| version.clone());
    let target = if swift {
        Some(observe_swift_target(platform, &output)?)
    } else {
        None
    };
    Ok(SystemToolProof {
        declared: pin.clone(),
        observed_version: version,
        observed_build: build,
        observed_sdk_version: sdk_version,
        sdk_stdout_digest: sdk_output
            .as_ref()
            .map(|(_, output)| digest_b3(&output.stdout)),
        sdk_stderr_digest: sdk_output
            .as_ref()
            .map(|(_, output)| digest_b3(&output.stderr)),
        observed_target: target,
        executable,
        launcher: launcher.to_owned(),
        developer_dir: directory.to_owned(),
        developer_stdout_digest: digest_b3(&selection.stdout),
        developer_stderr_digest: digest_b3(&selection.stderr),
        stdout_digest: digest_b3(&output.stdout),
        stderr_digest: digest_b3(&output.stderr),
        discovery_stdout_digest: discovery.as_ref().map(|o| digest_b3(&o.stdout)),
        discovery_stderr_digest: discovery.as_ref().map(|o| digest_b3(&o.stderr)),
    })
}

fn observe_swift_target(
    platform: CheckPlatform,
    output: &ProcessOutput,
) -> Result<String, MiseError> {
    let text = strict_text(&output.stdout)?;
    let target = text
        .lines()
        .nth(1)
        .ok_or_else(|| invalid("system_tools", "missing_swift_target"))?;
    let prefix = if platform == CheckPlatform::MacosArm64 {
        "Target: arm64-apple-macosx"
    } else {
        "Target: x86_64-apple-macosx"
    };
    if target
        .strip_prefix(prefix)
        .is_none_or(|value| !valid_version(value))
    {
        return Err(invalid("system_tools", "swift_target_mismatch"));
    }
    target
        .strip_prefix("Target: ")
        .map(str::to_owned)
        .ok_or_else(|| invalid("system_tools", "invalid_target"))
}

fn observe_xcode_sdk(
    pin: &CheckSystemTool,
    directory: &str,
    deadline: CheckDeadline,
) -> Result<(String, ProcessOutput), MiseError> {
    let output = probe(
        "/usr/bin/xcrun",
        &["--sdk", "macosx", "--show-sdk-version"],
        Some(directory),
        deadline,
    )?;
    let text = strict_text(&output.stdout)?.trim_end_matches('\n');
    if output.stdout != format!("{text}\n").as_bytes()
        || !valid_version(text)
        || pin.sdk_version.as_deref() != Some(text)
    {
        return Err(invalid("system_tools", "xcode_sdk_mismatch"));
    }
    Ok((text.to_owned(), output))
}

#[path = "check_system_tools_parse.rs"]
mod parse;
pub use parse::parse_system_tool_version;
use parse::{absolute_identity, valid_build, valid_proof_target, valid_version};

#[cfg(test)]
#[path = "check_system_tools_tests.rs"]
mod tests;

fn canonical_executable(value: &str) -> Result<String, MiseError> {
    let path = Path::new(value)
        .canonicalize()
        .map_err(|e| invalid("system_tools", e.to_string()))?;
    if !path.is_file() {
        return Err(invalid("system_tools", "missing_executable"));
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("system_tools", "invalid_executable_utf8"))
}

fn strict_text(bytes: &[u8]) -> Result<&str, MiseError> {
    std::str::from_utf8(bytes).map_err(|_| invalid("system_tools", "invalid_utf8"))
}

fn probe(
    program: &str,
    args: &[&str],
    directory: Option<&str>,
    deadline: CheckDeadline,
) -> Result<ProcessOutput, MiseError> {
    let mut env: Vec<_> = [("PATH", "/usr/bin:/bin:/usr/sbin:/sbin"), ("LC_ALL", "C")]
        .into_iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect();
    if let Some(directory) = directory {
        env.push((OsString::from("DEVELOPER_DIR"), OsString::from(directory)));
    }
    let result = IsolatedCommand::qualified_check_probe(
        OsString::from(program),
        args.iter().map(OsString::from).collect(),
        env,
    )
    .run_until(64 * 1024, deadline)?;
    strict_text(&result.stdout)?;
    if !strict_text(&result.stderr)?.is_empty() || !result.success {
        return Err(invalid("system_tools", "native_probe_failed"));
    }
    Ok(result)
}
