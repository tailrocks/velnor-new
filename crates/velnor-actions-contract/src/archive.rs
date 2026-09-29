//! Nextest archive identity inputs (par §8).
//!
//! The archive identity MUST include source, package, target, features,
//! profile, toolchain, runtime, Nextest version, archive format, plus
//! explicit platform and config digests.

use serde::Serialize;

use crate::cachekey::validate_semantic_text;
use crate::canonical::{canonical_json_bytes, digest_b3, validate_digest};
use crate::errors::ContractError;

/// Archive identity inputs (par §8).
#[derive(Debug, Clone, Serialize)]
pub struct ArchiveInputs {
    /// Source/input digest.
    pub source_digest: String,
    /// Cargo package name (display only, part of identity here).
    pub package: String,
    /// Execution target (`host` or triple).
    pub target: String,
    /// Sorted enabled features.
    pub features: Vec<String>,
    /// Build profile.
    pub profile: String,
    /// Toolchain identity digest.
    pub toolchain_id: String,
    /// Linker/runtime requirements.
    pub runtime: String,
    /// Exact Nextest version.
    pub test_runner: String,
    /// Archive format.
    pub format: String,
    /// Platform identity digest (explicit, not via runtime/profile).
    pub platform_id: String,
    /// Cargo config digest (explicit, not via target).
    pub config_digest: String,
}

/// Compute the archive identity digest over the canonical inputs.
/// # Errors
pub fn archive_id(inputs: &ArchiveInputs) -> Result<String, ContractError> {
    validate_digest(&inputs.source_digest)?;
    validate_digest(&inputs.toolchain_id)?;
    validate_digest(&inputs.platform_id)?;
    validate_digest(&inputs.config_digest)?;
    for (field, value) in [
        ("package", inputs.package.as_str()),
        ("target", inputs.target.as_str()),
        ("profile", inputs.profile.as_str()),
        ("runtime", inputs.runtime.as_str()),
        ("test_runner", inputs.test_runner.as_str()),
        ("format", inputs.format.as_str()),
    ] {
        validate_semantic_text(field, value)?;
    }
    let mut features = inputs.features.clone();
    features.sort();
    if features != inputs.features {
        return Err(ContractError::identity(
            "archive.features",
            "must_be_sorted",
        ));
    }
    for feature in &inputs.features {
        validate_semantic_text("archive.feature", feature)?;
    }
    Ok(digest_b3(&canonical_json_bytes(inputs)?))
}
