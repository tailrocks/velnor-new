use super::types::{QualificationRuntimeIdentity, QualificationRuntimeIdentityRequirements};
use crate::errors::ContractError;

pub(super) fn validate_tool_observations(
    requirements: &QualificationRuntimeIdentityRequirements,
    evidence: &QualificationRuntimeIdentity,
) -> Result<(), ContractError> {
    use crate::cachekey::validate_semantic_text;

    if evidence.tools.len() != requirements.toolchain_inputs.tools.len()
        || requirements
            .toolchain_inputs
            .tools
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        || requirements
            .toolchain_inputs
            .components
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity.tools",
            "toolchain_inputs_not_canonical",
        ));
    }
    for (expected, observed) in requirements
        .toolchain_inputs
        .tools
        .iter()
        .zip(&evidence.tools)
    {
        let (name, version) = expected.rsplit_once('@').ok_or_else(|| {
            ContractError::identity("qualification.runtime_identity.tools", "bad_plan_selector")
        })?;
        if name.is_empty()
            || version.is_empty()
            || observed.selector != *expected
            || observed.reported_version != version
        {
            return Err(ContractError::identity(
                "qualification.runtime_identity.tools",
                "pinned_tool_observation_mismatch",
            ));
        }
        validate_semantic_text("runtime_tool.selector", &observed.selector)?;
        validate_semantic_text("runtime_tool.version", &observed.reported_version)?;
        validate_sha256_hex(&observed.executable_sha256)?;
    }
    Ok(())
}

fn validate_sha256_hex(value: &str) -> Result<(), ContractError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(ContractError::identity(
            "qualification.runtime_identity.tool_sha256",
            "invalid_digest",
        ))
    }
}
