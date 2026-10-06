//! Admission bound for the exact named-check execution proof transport.

/// Maximum source or task-projection bytes admitted for one named check.
pub const MAX_CHECK_SOURCE_BYTES: usize = 8 * 1024 * 1024;
/// Maximum bytes in any parsed native system-tool version component string.
pub const MAX_CHECK_SYSTEM_VERSION_BYTES: usize = 128;
use super::{MiseCheck, QualifiedTool, QualifiedToolPlatform};
use crate::errors::ContractError;
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

/// Maximum serialized helper receipt accepted by the existing matrix artifact gate.
pub const MAX_CHECK_EXECUTION_RECEIPT_BYTES: usize = 1 << 20;
/// Maximum stdout retained for one qualified executable probe.
pub const MAX_CHECK_QUALIFIED_PROBE_CAPTURE_BYTES: usize = 8 * 1024 + 1;
/// Maximum declared exact version output before an optional trailing newline.
pub const MAX_CHECK_QUALIFIED_PROBE_EXPECTED_BYTES: usize = 8 * 1024;
/// Maximum stdout or stderr retained for the short Docker identity probes.
pub const MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES: usize = 8 * 1024;
/// Maximum stdout or stderr retained for the structured daemon JSON probe.
pub const MAX_CHECK_CONTAINER_DAEMON_CAPTURE_BYTES: usize = 4 * 1024;
/// Maximum stdout or stderr retained for the complete application Info.plist JSON.
pub const MAX_CHECK_CONTAINER_APP_INFO_CAPTURE_BYTES: usize = 16 * 1024;
/// Maximum stdout or stderr retained for a signed identity observation.
pub const MAX_CHECK_CONTAINER_IDENTITY_CAPTURE_BYTES: usize = 2 * 1024;
/// Maximum stdout or stderr retained for a quiet signature verification.
pub const MAX_CHECK_CONTAINER_APP_VERIFY_CAPTURE_BYTES: usize = 1024;
/// Maximum path bytes retained in an execution receipt.
pub const MAX_CHECK_CONTAINER_PATH_BYTES: usize = 1024;
/// Maximum runtime inventory entries admitted into one check receipt.
pub const MAX_CHECK_CONTAINER_RUNTIME_ENTRIES: usize = 64;
/// Maximum relative path bytes retained for one runtime inventory entry.
pub const MAX_CHECK_CONTAINER_RUNTIME_ENTRY_PATH_BYTES: usize = 256;

const RECEIPT_FIXED_BYTES: usize = 32 * 1024;
const RECEIPT_CONTAINER_FIXED_BYTES: usize = 16 * 1024;
const RECEIPT_PATH_FIELDS: usize = 32;

/// Reject a check whose worst-case receipt cannot fit through the established report reader.
pub(super) fn validate_check_budget(
    check: &MiseCheck,
    tools: &[QualifiedTool],
    file: &str,
    key: &str,
) -> Result<(), ContractError> {
    if check_execution_receipt_upper_bound(check, tools)? > MAX_CHECK_EXECUTION_RECEIPT_BYTES {
        return Err(ContractError::config(
            file,
            format!("{key}.tools"),
            "check_execution_receipt_budget_exceeded",
        ));
    }
    Ok(())
}

/// Compute the worst-case serialized named-check proof size from its declared closure.
/// # Errors
/// Returns an error only if the checked size arithmetic or serialization cannot be represented.
pub fn check_execution_receipt_upper_bound(
    check: &MiseCheck,
    tools: &[QualifiedTool],
) -> Result<usize, ContractError> {
    let mut budget = RECEIPT_FIXED_BYTES;
    if let Some(evidence) = &check.evidence {
        budget = add(
            budget,
            evidence.expected_scenarios.len().saturating_mul(160),
        )?;
    }
    if let Some(profile) = &check.runner.container {
        budget = add(budget, container_budget(profile)?)?;
    }
    budget = add(budget, system_tools_budget(check)?)?;
    let by_id: BTreeMap<_, _> = tools.iter().map(|tool| (tool.id.as_str(), tool)).collect();
    let mut closure = BTreeSet::new();
    let mut pending: Vec<_> = check.tools.iter().map(String::as_str).collect();
    while let Some(id) = pending.pop() {
        if !closure.insert(id) {
            continue;
        }
        let Some(tool) = by_id.get(id) else {
            continue;
        };
        pending.extend(tool.depends_on.iter().map(String::as_str));
    }
    for id in closure {
        let Some(tool) = by_id.get(id) else {
            continue;
        };
        if let Some(platform) = tool
            .platforms
            .iter()
            .find(|platform| platform.platform == check.runner.platform)
        {
            budget = add(budget, qualified_tool_budget(tool, platform)?)?;
        }
    }
    Ok(budget)
}

fn system_tools_budget(check: &MiseCheck) -> Result<usize, ContractError> {
    let path = format!("/{}", "\u{1}".repeat(MAX_CHECK_CONTAINER_PATH_BYTES - 1));
    let version = format!(
        "{}99",
        "9.".repeat((MAX_CHECK_SYSTEM_VERSION_BYTES - 2) / 2)
    );
    let proofs: Vec<_> = check
        .system_tools
        .iter()
        .map(|tool| {
            let swift = tool.kind == super::CheckSystemToolKind::Swift;
            let prefix = if check.runner.platform == super::CheckPlatform::MacosArm64 {
                "arm64-apple-macosx"
            } else {
                "x86_64-apple-macosx"
            };
            json!({
                "declared": tool,
                "observed_version": tool.version,
                "observed_build": tool.build,
                "observed_target": swift.then(|| format!("{prefix}{version}")),
                "executable": if swift { path.clone() } else { "/usr/bin/xcodebuild".to_owned() },
                "launcher": if swift { "/usr/bin/xcrun" } else { "/usr/bin/xcodebuild" },
                "developer_dir": path,
                "developer_stdout_digest": format!("b3-{}", "0".repeat(64)),
                "developer_stderr_digest": format!("b3-{}", "0".repeat(64)),
                "stdout_digest": format!("b3-{}", "0".repeat(64)),
                "stderr_digest": format!("b3-{}", "0".repeat(64)),
                "discovery_stdout_digest": swift.then(|| format!("b3-{}", "0".repeat(64))),
                "discovery_stderr_digest": swift.then(|| format!("b3-{}", "0".repeat(64))),
            })
        })
        .collect();
    encoded_len(&proofs)
}

fn qualified_tool_budget(
    tool: &QualifiedTool,
    platform: &QualifiedToolPlatform,
) -> Result<usize, ContractError> {
    let worst_stdout = "\u{1}".repeat(MAX_CHECK_QUALIFIED_PROBE_CAPTURE_BYTES);
    let worst_path = format!("/{}", "\u{1}".repeat(MAX_CHECK_CONTAINER_PATH_BYTES - 1));
    let base = json!({
        "id": tool.id,
        "version": tool.version,
        "platform": platform.platform,
        "definition_digest": "0".repeat(64),
        "artifacts": &platform.artifacts,
        "dependency_artifacts": &platform.dependency_artifacts,
        "install_tree_sha256": platform.install_tree_sha256,
        "executables": [],
    });
    let mut bytes = encoded_len(&base)?;
    for (index, executable) in platform.executables.iter().enumerate() {
        let proof = json!({
            "tool_id": tool.id,
            "tool_version": tool.version,
            "platform": platform.platform,
            "declared": executable,
            "observed": {
                "name": executable.name,
                "path": &worst_path,
                "sha256": executable.sha256,
            },
            "stdout": &worst_stdout,
            "stdout_digest": "0".repeat(64),
            "stderr_digest": "0".repeat(64),
        });
        bytes = add(bytes, encoded_len(&proof)?)?;
        if index > 0 {
            bytes = add(bytes, 1)?;
        }
    }
    Ok(bytes)
}

fn container_budget(profile: &super::HostContainerProfile) -> Result<usize, ContractError> {
    use super::HostContainerProfile;
    let profile_bytes = encoded_len(profile)?;
    let mut bytes = RECEIPT_CONTAINER_FIXED_BYTES;
    bytes = add(bytes, profile_bytes.saturating_mul(3))?;
    bytes = add(
        bytes,
        RECEIPT_PATH_FIELDS
            .saturating_mul(MAX_CHECK_CONTAINER_PATH_BYTES)
            .saturating_mul(2),
    )?;
    bytes = add(
        bytes,
        MAX_CHECK_CONTAINER_RUNTIME_ENTRIES
            .saturating_mul(MAX_CHECK_CONTAINER_RUNTIME_ENTRY_PATH_BYTES * 2 + 128),
    )?;
    let mut stream_caps = [
        MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES,
        MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES,
        MAX_CHECK_CONTAINER_DAEMON_CAPTURE_BYTES,
    ]
    .into_iter()
    .sum::<usize>();
    let probe_count: usize = match profile {
        HostContainerProfile::Docker { .. } => 3,
        HostContainerProfile::OrbStack { .. } => {
            stream_caps = stream_caps
                .saturating_add(MAX_CHECK_CONTAINER_APP_INFO_CAPTURE_BYTES)
                .saturating_add(3 * MAX_CHECK_CONTAINER_IDENTITY_CAPTURE_BYTES)
                .saturating_add(3 * MAX_CHECK_CONTAINER_APP_VERIFY_CAPTURE_BYTES)
                .saturating_add(2 * MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES);
            12
        }
    };
    // Each output has stdout and stderr, each JSON-escaped at most 2x after
    // the runner rejects control bytes other than line whitespace.
    bytes = add(bytes, stream_caps.saturating_mul(8))?;
    bytes = add(bytes, probe_count.saturating_mul(2 * 512))?;
    Ok(bytes)
}

fn encoded_len<T: Serialize>(value: &T) -> Result<usize, ContractError> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|_| ContractError::config(".velnor/config.toml", "checks", "receipt_size"))
}

fn add(left: usize, right: usize) -> Result<usize, ContractError> {
    left.checked_add(right)
        .ok_or_else(|| ContractError::config(".velnor/config.toml", "checks", "receipt_size"))
}

#[cfg(test)]
mod tests;
