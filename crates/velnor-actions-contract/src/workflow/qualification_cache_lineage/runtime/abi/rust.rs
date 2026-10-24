use super::super::types::{
    QualificationRuntimeAbiObservation, QualificationRuntimeIdentity,
    QualificationRuntimeIdentityRequirements, QualificationRuntimePlatform,
};
use super::shared::{
    exact_one_prefixed_line, expected_host_triple, pinned_version_for_family, target_os_for_triple,
    validate_multiline_fact,
};
use crate::errors::ContractError;

pub(super) fn validate_rust_observation(
    requirements: &QualificationRuntimeIdentityRequirements,
    evidence: &QualificationRuntimeIdentity,
) -> Result<(), ContractError> {
    let rust = evidence.abi.iter().find_map(|fact| match fact {
        QualificationRuntimeAbiObservation::Rust {
            host,
            target,
            version_verbose,
            target_cfg,
            installed_components,
        } => Some((
            host,
            target,
            version_verbose,
            target_cfg,
            installed_components,
        )),
        _ => None,
    });
    let has_rust = requirements
        .toolchain_inputs
        .tools
        .iter()
        .any(|selector| selector.starts_with("rust@"));
    if has_rust != rust.is_some() {
        return Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            "rust_fact_presence_mismatch",
        ));
    }
    if let Some((host, target, version_verbose, target_cfg, components)) = rust {
        validate_rust_fact(
            requirements,
            &evidence.platform,
            host,
            target,
            version_verbose,
            target_cfg,
            components,
        )?;
    }
    Ok(())
}

fn validate_rust_fact(
    requirements: &QualificationRuntimeIdentityRequirements,
    platform: &QualificationRuntimePlatform,
    host: &str,
    target: &str,
    version_verbose: &str,
    target_cfg: &str,
    installed_components: &[String],
) -> Result<(), ContractError> {
    use crate::cachekey::validate_semantic_text;

    let pinned_version = pinned_version_for_family(requirements, "rust")?;
    let expected_host = expected_host_triple(platform).ok_or_else(|| {
        ContractError::identity(
            "qualification.runtime_identity.abi",
            "rust_host_platform_unsupported",
        )
    })?;
    let expected_target_arch = requirements
        .target
        .split('-')
        .next()
        .ok_or_else(|| ContractError::identity("runtime_abi.rust_target", "invalid"))?;
    let expected_target_os = target_os_for_triple(&requirements.target)
        .ok_or_else(|| ContractError::identity("runtime_abi.rust_target", "unsupported"))?;
    let cfg_lines = target_cfg.lines().collect::<Vec<_>>();
    if target != requirements.target
        || !exact_one_prefixed_line(version_verbose, "release: ", pinned_version)
        || !exact_one_prefixed_line(version_verbose, "host: ", host)
        || host != expected_host
        || !cfg_lines.contains(&format!("target_arch=\"{expected_target_arch}\"").as_str())
        || !cfg_lines.contains(&format!("target_os=\"{expected_target_os}\"").as_str())
        || cfg_lines.windows(2).any(|pair| pair[0] >= pair[1])
        || installed_components
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            "rust_fact_mismatch",
        ));
    }
    validate_semantic_text("runtime_abi.rust_host", host)?;
    validate_multiline_fact("runtime_abi.rustc_verbose", version_verbose)?;
    validate_multiline_fact("runtime_abi.rustc_cfg", target_cfg)?;
    for component in installed_components {
        validate_semantic_text("runtime_abi.rust_component", component)?;
    }
    let missing_component = requirements
        .toolchain_inputs
        .components
        .iter()
        .any(|component| {
            matches!(component.as_str(), "clippy" | "rustfmt")
                && !installed_components.contains(component)
        });
    if missing_component {
        return Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            "rust_component_missing",
        ));
    }
    Ok(())
}
