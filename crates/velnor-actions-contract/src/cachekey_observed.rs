//! Strict identity derivation from observed runner facts.

use super::{PlatformInputs, platform_id};
use crate::cachekey::validate_semantic_text;
use crate::errors::ContractError;
use crate::freshness::{RunnerImageEvidence, UNOBSERVED_IMAGE_VALUE};

/// Maximum encoded size for one observed platform fact.
pub const MAX_OBSERVED_PLATFORM_FACT_BYTES: usize = 256;

/// Compute the canonical platform identity only from complete observed facts.
///
/// The planned identity may contain explicitly unobserved image values. This
/// function is the separate runtime admission path and rejects that marker in
/// every field, malformed text, and oversized values before hashing.
/// # Errors
pub fn observed_platform_id(inputs: &PlatformInputs) -> Result<String, ContractError> {
    for (field, value) in [
        ("os", inputs.os.as_str()),
        ("arch", inputs.arch.as_str()),
        ("runs_on", inputs.runs_on.as_str()),
        ("target", inputs.target.as_str()),
    ] {
        validate_observed_fact(field, value)?;
    }
    let evidence = RunnerImageEvidence::observed(&inputs.image_os, &inputs.image_version)?;
    validate_observed_fact("image_os", &evidence.image_os)?;
    validate_observed_fact("image_version", &evidence.image_version)?;
    platform_id(inputs)
}

/// Validate a bounded, non-marker runtime fact.
fn validate_observed_fact(field: &'static str, value: &str) -> Result<(), ContractError> {
    validate_semantic_text(field, value)?;
    if value == UNOBSERVED_IMAGE_VALUE {
        return Err(ContractError::identity(field, "unobserved_marker"));
    }
    if value.len() > MAX_OBSERVED_PLATFORM_FACT_BYTES {
        return Err(ContractError::identity(field, "fact_too_long"));
    }
    Ok(())
}
