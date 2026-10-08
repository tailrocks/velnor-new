//! One-read validated host configuration and its exact source digest.

use std::path::Path;

use sha2::{Digest, Sha256};
use velnor_runner_docker_spec::RunnerImageProfile;
use velnor_runner_host_config::{
    HostConfig, HostPlatform, ScaleSetBinding, read_host_config_bytes,
};

use crate::HostError;

/// Immutable validated view of one protected configuration file.
///
/// The digest is SHA-256 over the exact byte buffer parsed and validated by
/// this loader. Fields are private so callers cannot pair a digest with a
/// different configuration or binding.
#[derive(Debug)]
pub struct ValidatedHostConfigSnapshot {
    config: HostConfig,
    policy_digest: String,
    binding: ScaleSetBinding,
    runner_image_profile: Option<RunnerImageProfile>,
    platform: HostPlatform,
}

impl ValidatedHostConfigSnapshot {
    /// Parsed, validated configuration retained from the protected bytes.
    #[must_use]
    pub const fn config(&self) -> &HostConfig {
        &self.config
    }

    /// Lowercase SHA-256 of the exact protected bytes parsed for this snapshot.
    #[must_use]
    pub fn policy_digest(&self) -> &str {
        &self.policy_digest
    }

    /// Exact typed registration scope and target/group/set binding.
    #[must_use]
    pub const fn scale_set_binding(&self) -> &ScaleSetBinding {
        &self.binding
    }

    /// Resolved immutable image profile, absent when no approved profile can be
    /// resolved. In particular, Ubuntu 26 config remains usable for cleanup and
    /// reconciliation while runner admission/start stays unavailable.
    #[must_use]
    pub const fn runner_image_profile(&self) -> Option<RunnerImageProfile> {
        self.runner_image_profile
    }

    /// Host backend validated for this snapshot.
    #[must_use]
    pub const fn platform(&self) -> HostPlatform {
        self.platform
    }
}

/// Read and validate one protected configuration snapshot.
///
/// The protected loader returns exact bytes once. This function hashes and
/// parses that same buffer, then derives the typed binding and immutable image
/// profile without reopening the path.
///
/// Missing legacy macOS configuration is `Ok(None)`. A missing Linux system
/// configuration is an error because the Linux backend requires an installed
/// package-owned file.
///
/// # Errors
///
/// Returns [`HostError::Config`] when the file is missing on Linux, malformed,
/// unsafe, for another platform, or has an invalid binding/profile.
pub fn read_validated_host_config_snapshot(
    path: &Path,
    platform: HostPlatform,
) -> Result<Option<ValidatedHostConfigSnapshot>, HostError> {
    snapshot_from_optional_bytes(read_host_config_bytes(path, platform)?, platform)
}

fn snapshot_from_optional_bytes(
    bytes: Option<Vec<u8>>,
    platform: HostPlatform,
) -> Result<Option<ValidatedHostConfigSnapshot>, HostError> {
    let Some(bytes) = bytes else {
        return if platform == HostPlatform::Linux {
            Err(HostError::Config)
        } else {
            Ok(None)
        };
    };
    snapshot_from_bytes(&bytes, platform).map(Some)
}

fn snapshot_from_bytes(
    bytes: &[u8],
    platform: HostPlatform,
) -> Result<ValidatedHostConfigSnapshot, HostError> {
    let policy_digest = lower_hex(&Sha256::digest(bytes));
    let text = std::str::from_utf8(bytes).map_err(|_| HostError::Config)?;
    let config = HostConfig::parse(text)?;
    config.validate_for_host(platform)?;
    let binding = config.scale_set_binding()?;
    // HostConfig validates Linux to the exact required Ubuntu 26 selector.
    // No corresponding immutable RunnerImageProfile is currently approved,
    // so retain the binding but expose no runnable profile to start callers.
    let runner_image_profile = None;
    Ok(ValidatedHostConfigSnapshot {
        config,
        policy_digest,
        binding,
        runner_image_profile,
        platform,
    })
}

fn lower_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
mod tests;
