//! Canonical runner-platform identity inputs and derivation.

use serde::{Deserialize, Serialize};

use crate::cachekey::validate_semantic_text;
use crate::canonical::{canonical_json_bytes, digest_b3};
use crate::errors::ContractError;

/// Platform identity inputs, including runner image metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformInputs {
    /// Operating system name.
    pub os: String,
    /// CPU architecture.
    pub arch: String,
    /// Exact literal `runs-on` label.
    pub runs_on: String,
    /// Runner `ImageOS` value; `unknown` when unobserved.
    pub image_os: String,
    /// Runner `ImageVersion` value; `unknown` when unobserved.
    pub image_version: String,
    /// Execution target (`host` or triple).
    pub target: String,
}

/// Compute `platform_id` over OS/arch/label/image/target.
/// # Errors
pub fn platform_id(inputs: &PlatformInputs) -> Result<String, ContractError> {
    for (field, value) in [
        ("os", inputs.os.as_str()),
        ("arch", inputs.arch.as_str()),
        ("runs_on", inputs.runs_on.as_str()),
        ("image_os", inputs.image_os.as_str()),
        ("image_version", inputs.image_version.as_str()),
        ("target", inputs.target.as_str()),
    ] {
        validate_semantic_text(field, value)?;
    }
    Ok(digest_b3(&canonical_json_bytes(inputs)?))
}

#[path = "cachekey_observed.rs"]
mod observed;
pub use observed::{MAX_OBSERVED_PLATFORM_FACT_BYTES, observed_platform_id};
