use super::super::types::{
    QualificationRuntimeAbiObservation, QualificationRuntimeIdentity,
    QualificationRuntimeIdentityRequirements, QualificationRuntimePlatform,
};
use crate::errors::ContractError;

pub(super) fn abi_families_in_order(
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

pub(super) fn pinned_version_for_family<'a>(
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

pub(super) fn normalize_os(value: &str) -> Option<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "linux" => Some("linux"),
        "darwin" | "macos" | "mac os" => Some("macos"),
        "windows" | "win32" => Some("windows"),
        _ => None,
    }
}

pub(super) fn contains_exact_version(output: &str, expected: &str) -> bool {
    output
        .split(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | '+'))
        })
        .any(|token| token == expected || token.strip_prefix('v') == Some(expected))
}

pub(super) fn exact_one_prefixed_line(output: &str, prefix: &str, expected: &str) -> bool {
    let matches = output
        .lines()
        .filter_map(|line| line.strip_prefix(prefix))
        .collect::<Vec<_>>();
    matches.as_slice() == [expected]
}

pub(super) fn normalize_arch(value: &str) -> Option<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "x64" | "x86_64" | "amd64" => Some("x86_64"),
        "arm64" | "aarch64" => Some("aarch64"),
        "x86" | "i386" | "i686" => Some("i686"),
        _ => None,
    }
}

pub(super) fn expected_host_triple(
    platform: &QualificationRuntimePlatform,
) -> Option<&'static str> {
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

pub(super) fn target_os_for_triple(target: &str) -> Option<&'static str> {
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

pub(super) fn platform_matches_target(platform: &str, target: &str) -> bool {
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

pub(super) fn require_abi_for_pins(
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

pub(super) fn validate_multiline_fact(
    field: &'static str,
    value: &str,
) -> Result<(), ContractError> {
    if value.is_empty() || value.len() > 65_536 || value.contains('\0') || value.contains('\r') {
        return Err(ContractError::identity(field, "invalid_fact"));
    }
    Ok(())
}
