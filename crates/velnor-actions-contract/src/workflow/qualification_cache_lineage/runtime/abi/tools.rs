use super::super::types::{
    QualificationRuntimeAbiObservation, QualificationRuntimeIdentity,
    QualificationRuntimeIdentityRequirements,
};
use super::shared::{
    contains_exact_version, normalize_arch, normalize_os, pinned_version_for_family,
    platform_matches_target, validate_multiline_fact,
};
use crate::errors::ContractError;

pub(super) fn validate_tool_abi_facts(
    requirements: &QualificationRuntimeIdentityRequirements,
    evidence: &QualificationRuntimeIdentity,
) -> Result<(), ContractError> {
    for fact in &evidence.abi {
        match fact {
            QualificationRuntimeAbiObservation::Java {
                version_output,
                arch,
            } => validate_java_fact(requirements, evidence, version_output, arch)?,
            QualificationRuntimeAbiObservation::Node {
                version_output,
                module_abi,
                napi,
                platform,
                arch,
            } => validate_node_fact(
                requirements,
                evidence,
                version_output,
                module_abi,
                napi,
                platform,
                arch,
            )?,
            QualificationRuntimeAbiObservation::OpenTofu {
                version_output,
                platform,
            } => validate_tofu_fact(requirements, version_output, platform)?,
            QualificationRuntimeAbiObservation::Host { .. }
            | QualificationRuntimeAbiObservation::Rust { .. } => {}
        }
    }
    Ok(())
}

fn validate_java_fact(
    requirements: &QualificationRuntimeIdentityRequirements,
    evidence: &QualificationRuntimeIdentity,
    version_output: &str,
    arch: &str,
) -> Result<(), ContractError> {
    use crate::cachekey::validate_semantic_text;

    let pinned_version = pinned_version_for_family(requirements, "java")?;
    validate_semantic_text("runtime_abi.java_arch", arch)?;
    validate_multiline_fact("runtime_abi.java_version", version_output)?;
    if !contains_exact_version(version_output, pinned_version)
        || normalize_arch(arch) != normalize_arch(&evidence.platform.arch)
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            "java_fact_mismatch",
        ));
    }
    Ok(())
}

fn validate_node_fact(
    requirements: &QualificationRuntimeIdentityRequirements,
    evidence: &QualificationRuntimeIdentity,
    version_output: &str,
    module_abi: &str,
    napi: &str,
    platform: &str,
    arch: &str,
) -> Result<(), ContractError> {
    use crate::cachekey::validate_semantic_text;

    let pinned_version = pinned_version_for_family(requirements, "node")?;
    for (field, value) in [
        ("runtime_abi.node_module_abi", module_abi),
        ("runtime_abi.node_napi", napi),
        ("runtime_abi.node_platform", platform),
        ("runtime_abi.node_arch", arch),
    ] {
        validate_semantic_text(field, value)?;
    }
    validate_multiline_fact("runtime_abi.node_version", version_output)?;
    if !contains_exact_version(version_output, pinned_version)
        || normalize_os(platform) != normalize_os(&evidence.platform.os)
        || normalize_arch(arch) != normalize_arch(&evidence.platform.arch)
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            "node_fact_mismatch",
        ));
    }
    Ok(())
}

fn validate_tofu_fact(
    requirements: &QualificationRuntimeIdentityRequirements,
    version_output: &str,
    platform: &str,
) -> Result<(), ContractError> {
    use crate::cachekey::validate_semantic_text;

    let pinned_version = pinned_version_for_family(requirements, "open_tofu")?;
    validate_semantic_text("runtime_abi.tofu_platform", platform)?;
    validate_multiline_fact("runtime_abi.tofu_version", version_output)?;
    if !contains_exact_version(version_output, pinned_version)
        || !platform_matches_target(platform, &requirements.target)
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            "open_tofu_fact_mismatch",
        ));
    }
    Ok(())
}
