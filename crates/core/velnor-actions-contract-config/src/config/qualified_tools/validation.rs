//! Closed declaration admission and explicit prerequisite graph validation.
use super::{
    ContractError, QualifiedCargoInstallation, QualifiedTool, QualifiedToolBackend,
    QualifiedToolOptions,
};
use std::collections::{BTreeMap, BTreeSet};

mod probes;
mod sources;

pub(super) fn safe_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

pub(super) fn safe_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value
            .split('/')
            .all(|part| safe_name(part) && part != "." && part != "..")
}

pub(super) fn sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && value.bytes().any(|byte| byte != b'0')
}

fn exact_version(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

fn sorted_names(values: &[String]) -> bool {
    values.iter().all(|value| safe_name(value)) && values.windows(2).all(|pair| pair[0] < pair[1])
}

pub(super) fn validate_tool(
    tool: &QualifiedTool,
    file: &str,
    key: &str,
) -> Result<(), ContractError> {
    let bad = |problem| ContractError::config(file, key, problem);
    if !safe_name(&tool.id) || !exact_version(&tool.version) {
        return Err(bad("invalid_qualified_tool_identity"));
    }
    if !sorted_names(&tool.depends_on) || tool.depends_on.iter().any(|id| id == &tool.id) {
        return Err(bad("invalid_qualified_tool_dependencies"));
    }
    validate_options(tool, file, key)?;
    if tool.platforms.is_empty()
        || !tool
            .platforms
            .windows(2)
            .all(|pair| pair[0].platform < pair[1].platform)
    {
        return Err(bad(
            "qualified_tool_platforms_must_be_sorted_unique_nonempty",
        ));
    }
    for platform in &tool.platforms {
        sources::validate_platform_sources(tool, platform, file, key)?;
        if !sha256(&platform.install_tree_sha256)
            || platform.executables.is_empty()
            || !platform
                .executables
                .windows(2)
                .all(|pair| pair[0].name < pair[1].name)
        {
            return Err(bad("invalid_qualified_tool_installed_identity"));
        }
        let mut paths = BTreeSet::new();
        for executable in &platform.executables {
            if !safe_name(&executable.name)
                || !safe_path(&executable.path)
                || !sha256(&executable.sha256)
                || !paths.insert(&executable.path)
            {
                return Err(bad("invalid_qualified_tool_executable"));
            }
            probes::validate_probe(tool, platform.platform, executable, file, key)?;
        }
    }
    Ok(())
}

fn validate_options(tool: &QualifiedTool, file: &str, key: &str) -> Result<(), ContractError> {
    let bad = |problem| ContractError::config(file, key, problem);
    match (&tool.backend, &tool.options) {
        (
            QualifiedToolBackend::Core { tool: name },
            QualifiedToolOptions::Rust {
                components,
                targets,
            },
        ) if name == "rust" => {
            if !sorted_names(components) || !sorted_names(targets) {
                return Err(bad("invalid_qualified_rust_options"));
            }
        }
        (QualifiedToolBackend::Core { tool: name }, QualifiedToolOptions::Default)
            if matches!(name.as_str(), "node" | "bun") => {}
        (QualifiedToolBackend::Aqua { package }, QualifiedToolOptions::Default)
            if sources::safe_package(package, true) => {}
        (
            QualifiedToolBackend::Cargo { crate_name },
            QualifiedToolOptions::Cargo {
                default_features,
                features,
                installation,
            },
        ) if safe_name(crate_name) => {
            if !sorted_names(features) {
                return Err(bad("invalid_qualified_cargo_features"));
            }
            match installation {
                QualifiedCargoInstallation::Source { source_lock_sha256 }
                    if sha256(source_lock_sha256) => {}
                QualifiedCargoInstallation::Prebuilt { repository }
                    if *default_features
                        && features.is_empty()
                        && sources::safe_package(repository, false) => {}
                _ => return Err(bad("invalid_qualified_cargo_installation")),
            }
        }
        _ => return Err(bad("unsupported_qualified_backend_options")),
    }
    Ok(())
}

pub(super) fn validate_registry(tools: &[QualifiedTool], file: &str) -> Result<(), ContractError> {
    let bad = |problem| ContractError::config(file, "qualified_tools", problem);
    if tools.len() > 128 || !tools.windows(2).all(|pair| pair[0].id < pair[1].id) {
        return Err(bad("qualified_tools_must_be_sorted_unique"));
    }
    let by_id: BTreeMap<_, _> = tools.iter().map(|tool| (tool.id.as_str(), tool)).collect();
    for (index, tool) in tools.iter().enumerate() {
        tool.validate(file, &format!("qualified_tools[{index}]"))?;
        if tool
            .depends_on
            .iter()
            .any(|id| !by_id.contains_key(id.as_str()))
        {
            return Err(bad("unknown_qualified_tool_dependency"));
        }
    }
    let mut remaining: BTreeSet<_> = by_id.keys().copied().collect();
    let mut admitted = BTreeSet::new();
    while !remaining.is_empty() {
        let ready: Vec<_> = remaining
            .iter()
            .copied()
            .filter(|id| {
                by_id[id]
                    .depends_on
                    .iter()
                    .all(|dependency| admitted.contains(dependency.as_str()))
            })
            .collect();
        if ready.is_empty() {
            return Err(bad("qualified_tool_dependency_cycle"));
        }
        for id in ready {
            remaining.remove(id);
            admitted.insert(id);
        }
    }
    validate_prerequisites(tools, &by_id, file)
}

fn validate_prerequisites(
    tools: &[QualifiedTool],
    by_id: &BTreeMap<&str, &QualifiedTool>,
    file: &str,
) -> Result<(), ContractError> {
    for tool in tools {
        let dependencies: Vec<_> = tool
            .depends_on
            .iter()
            .filter_map(|id| by_id.get(id.as_str()).copied())
            .collect();
        let rust_self =
            matches!(&tool.backend, QualifiedToolBackend::Core { tool } if tool == "rust");
        let compilers = dependencies.iter().filter(|dependency| {
            matches!(&dependency.backend, QualifiedToolBackend::Core { tool } if tool == "rust")
        }).count();
        if tool.requires_compiler() && !rust_self && compilers != 1 {
            return Err(ContractError::config(
                file,
                "qualified_tools",
                "qualified_tool_requires_one_direct_rust_dependency",
            ));
        }
        for platform in &tool.platforms {
            if dependencies.iter().any(|dependency| {
                !dependency
                    .platforms
                    .iter()
                    .any(|candidate| candidate.platform == platform.platform)
            }) {
                return Err(ContractError::config(
                    file,
                    "qualified_tools",
                    "qualified_dependency_platform_missing",
                ));
            }
        }
    }
    Ok(())
}
