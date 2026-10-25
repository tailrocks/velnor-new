use serde::Serialize;

use super::{
    abi::validate_abi_observations,
    image::validate_image_identity,
    tools::validate_tool_observations,
    types::{QualificationRuntimeIdentity, QualificationRuntimeIdentityRequirements},
};
use crate::canonical::{canonical_json_bytes, digest_b3, validate_digest};
use crate::errors::ContractError;

#[derive(Serialize)]
struct RuntimeIdentityDigestInput<'a> {
    requirements: &'a QualificationRuntimeIdentityRequirements,
    observations: &'a QualificationRuntimeIdentity,
}

/// Check runtime observations against plan identity and derive their digest.
pub(crate) fn validate_runtime_identity(
    requirements: &QualificationRuntimeIdentityRequirements,
    evidence: &QualificationRuntimeIdentity,
) -> Result<String, ContractError> {
    use crate::cachekey::validate_semantic_text;

    let platform = &evidence.platform;
    if platform.runner_label != requirements.runner_label
        || platform.runs_on != requirements.runs_on
        || platform.target != requirements.target
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity",
            "runner_binding_mismatch",
        ));
    }
    for (field, value) in [
        ("runner_os", platform.os.as_str()),
        ("runner_arch", platform.arch.as_str()),
        ("runner_label", platform.runner_label.as_str()),
        ("runs_on", platform.runs_on.as_str()),
        ("target", platform.target.as_str()),
    ] {
        validate_semantic_text(field, value)?;
    }
    validate_image_identity(requirements, platform)?;
    if crate::cachekey::toolchain_id(&requirements.toolchain_inputs)? != requirements.toolchain_id
        || !requirements.require_abi
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity",
            "plan_toolchain_commitment_mismatch",
        ));
    }
    validate_digest(&requirements.cache_format_id)?;
    validate_digest(&requirements.driver_id)?;
    validate_tool_observations(requirements, evidence)?;
    validate_abi_observations(requirements, evidence)?;
    Ok(digest_b3(&canonical_json_bytes(
        &RuntimeIdentityDigestInput {
            requirements,
            observations: evidence,
        },
    )?))
}
