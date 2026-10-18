use super::types::{
    QualificationRuntimeAbiObservation, QualificationRuntimeIdentity,
    QualificationRuntimeIdentityRequirements, QualificationRuntimePlatform,
};
use crate::errors::ContractError;

pub(super) fn validate_abi_observations(
    requirements: &QualificationRuntimeIdentityRequirements,
    evidence: &QualificationRuntimeIdentity,
) -> Result<(), ContractError> {
    let families = abi_families_in_order(evidence)?;
    validate_host_fact(evidence)?;
    validate_rust_observation(requirements, evidence)?;
    validate_tool_abi_facts(requirements, evidence)?;
    require_abi_for_pins(requirements, &families)?;
    Ok(())
}

fn abi_families_in_order(
    evidence: &QualificationRuntimeIdentity,
) -> Result<std::collections::BTreeSet<&'static str>, ContractError> {
    let mut families = std::collections::BTreeSet::new();
    let mut previous_rank = None;
    for fact in &evidence.abi {
        let (family, rank) = match fact {
            QualificationRuntimeAbiObservation::Host { .. } => ("host", 0),
            QualificationRuntimeAbiObservation::Rust { .. } => ("rust", 1),
            QualificationRuntimeAbiObservation::Java { .. } => ("java", 2),
            QualificationRuntimeAbiObservation::Node { .. } => ("node", 3),
            QualificationRuntimeAbiObservation::OpenTofu { .. } => ("open_tofu", 4),
        };
        if !families.insert(family) || previous_rank.is_some_and(|previous| previous >= rank) {
            return Err(ContractError::identity(
                "qualification.runtime_identity.abi",
                "abi_facts_not_canonical",
            ));
        }
        previous_rank = Some(rank);
    }
    Ok(families)
}

fn validate_host_fact(evidence: &QualificationRuntimeIdentity) -> Result<(), ContractError> {
    use crate::cachekey::validate_semantic_text;

    let host = evidence
        .abi
        .iter()
        .find_map(|fact| match fact {
            QualificationRuntimeAbiObservation::Host {
                os,
                arch,
                kernel,
                libc,
            } => Some((os, arch, kernel, libc)),
            _ => None,
        })
        .ok_or_else(|| {
            ContractError::identity("qualification.runtime_identity.abi", "host_fact_missing")
        })?;
    if normalize_os(host.0) != normalize_os(&evidence.platform.os)
        || normalize_arch(host.1) != normalize_arch(&evidence.platform.arch)
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            "host_fact_mismatch",
        ));
    }
    validate_semantic_text("runtime_abi.kernel", host.2)?;
    if let Some(libc) = host.3 {
        validate_semantic_text("runtime_abi.libc", libc)?;
    }
    if normalize_os(&evidence.platform.os) == Some("linux")
        && evidence.platform.target.ends_with("-gnu")
        && host.3.is_none()
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            "linux_libc_fact_missing",
        ));
    }
    Ok(())
}

fn validate_rust_observation(
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

fn validate_tool_abi_facts(
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

fn pinned_version_for_family<'a>(
    requirements: &'a QualificationRuntimeIdentityRequirements,
    family: &str,
) -> Result<&'a str, ContractError> {
    let matches = requirements
        .toolchain_inputs
        .tools
        .iter()
        .filter_map(|selector| {
            let (name, version) = selector.rsplit_once('@')?;
            let matches = match family {
                "rust" => name == "rust",
                "java" => name == "java" || name.ends_with("/java"),
                "node" => matches!(name, "node" | "nodejs" | "bun"),
                "open_tofu" => name == "opentofu",
                _ => false,
            };
            matches.then_some(version)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [version] => Ok(*version),
        [] => Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            format!("{family}_tool_pin_missing"),
        )),
        _ => Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            format!("{family}_tool_pin_ambiguous"),
        )),
    }
}

fn normalize_os(value: &str) -> Option<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "linux" => Some("linux"),
        "darwin" | "macos" | "mac os" => Some("macos"),
        "windows" | "win32" => Some("windows"),
        _ => None,
    }
}

fn contains_exact_version(output: &str, expected: &str) -> bool {
    output
        .split(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | '+'))
        })
        .any(|token| token == expected || token.strip_prefix('v') == Some(expected))
}

fn exact_one_prefixed_line(output: &str, prefix: &str, expected: &str) -> bool {
    let matches = output
        .lines()
        .filter_map(|line| line.strip_prefix(prefix))
        .collect::<Vec<_>>();
    matches.as_slice() == [expected]
}

fn normalize_arch(value: &str) -> Option<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "x64" | "x86_64" | "amd64" => Some("x86_64"),
        "arm64" | "aarch64" => Some("aarch64"),
        "x86" | "i386" | "i686" => Some("i686"),
        _ => None,
    }
}

fn expected_host_triple(platform: &QualificationRuntimePlatform) -> Option<&'static str> {
    match (normalize_os(&platform.os)?, normalize_arch(&platform.arch)?) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Some("aarch64-pc-windows-msvc"),
        _ => None,
    }
}

fn target_os_for_triple(target: &str) -> Option<&'static str> {
    if target.contains("-linux-") {
        Some("linux")
    } else if target.ends_with("-darwin") {
        Some("macos")
    } else if target.contains("-windows-") {
        Some("windows")
    } else {
        None
    }
}

fn platform_matches_target(platform: &str, target: &str) -> bool {
    let Some(os) = target_os_for_triple(target) else {
        return false;
    };
    let Some(arch) = target.split('-').next().and_then(normalize_arch) else {
        return false;
    };
    platform.to_ascii_lowercase().replace('-', "_")
        == format!(
            "{}_{}",
            os,
            match arch {
                "x86_64" => "amd64",
                "aarch64" => "arm64",
                other => other,
            }
        )
}

fn require_abi_for_pins(
    requirements: &QualificationRuntimeIdentityRequirements,
    families: &std::collections::BTreeSet<&str>,
) -> Result<(), ContractError> {
    let has_java = requirements.toolchain_inputs.tools.iter().any(|selector| {
        selector
            .split('@')
            .next()
            .is_some_and(|name| name.ends_with("/java") || name == "java")
    });
    let has_node = requirements.toolchain_inputs.tools.iter().any(|selector| {
        selector
            .split('@')
            .next()
            .is_some_and(|name| matches!(name, "node" | "nodejs" | "bun"))
    });
    let has_tofu = requirements.toolchain_inputs.compile_driver == "tofu"
        || requirements
            .toolchain_inputs
            .tools
            .iter()
            .any(|selector| selector.starts_with("opentofu@"));
    for (required, family) in [
        (has_java, "java"),
        (has_node, "node"),
        (has_tofu, "open_tofu"),
    ] {
        if required != families.contains(family) {
            return Err(ContractError::identity(
                "qualification.runtime_identity.abi",
                format!(
                    "{family}_fact_{}",
                    if required { "missing" } else { "unrequested" }
                ),
            ));
        }
    }
    Ok(())
}

fn validate_multiline_fact(field: &'static str, value: &str) -> Result<(), ContractError> {
    if value.is_empty() || value.len() > 65_536 || value.contains('\0') || value.contains('\r') {
        return Err(ContractError::identity(field, "invalid_fact"));
    }
    Ok(())
}
