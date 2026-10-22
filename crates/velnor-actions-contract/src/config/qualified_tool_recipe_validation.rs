//! Shared metadata admission for hash-free recipes and complete installations.
use super::{
    ContractError, QualifiedCargoInstallation, QualifiedToolBackend, QualifiedToolOptions,
    QualifiedToolRecipe,
};
use std::collections::{BTreeMap, BTreeSet};

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

pub(super) fn exact_version(value: &str) -> bool {
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

pub(super) fn validate_recipe(
    recipe: &QualifiedToolRecipe,
    file: &str,
    key: &str,
) -> Result<(), ContractError> {
    let bad = |problem| ContractError::config(file, key, problem);
    if !safe_name(&recipe.id) || !exact_version(&recipe.version) {
        return Err(bad("invalid_qualified_tool_identity"));
    }
    if !sorted_names(&recipe.depends_on) || recipe.depends_on.iter().any(|id| id == &recipe.id) {
        return Err(bad("invalid_qualified_tool_dependencies"));
    }
    validate_options(recipe, file, key)?;
    if recipe.platforms.is_empty()
        || !recipe
            .platforms
            .windows(2)
            .all(|pair| pair[0].platform < pair[1].platform)
    {
        return Err(bad(
            "qualified_tool_platforms_must_be_sorted_unique_nonempty",
        ));
    }
    for platform in &recipe.platforms {
        super::sources::validate_platform_recipe_sources(recipe, platform, file, key)?;
        if platform.executables.is_empty()
            || !platform
                .executables
                .windows(2)
                .all(|pair| pair[0].name < pair[1].name)
        {
            return Err(bad("invalid_qualified_tool_executable_order"));
        }
        let mut paths = BTreeSet::new();
        for executable in &platform.executables {
            if !safe_name(&executable.name)
                || !safe_path(&executable.path)
                || !paths.insert(&executable.path)
            {
                return Err(bad("invalid_qualified_tool_executable"));
            }
            if matches!(
                executable.probe,
                super::QualifiedToolProbe::RustfmtVersion { .. }
            ) || (matches!(&recipe.backend, QualifiedToolBackend::Core { tool } if tool == "rust")
                && (matches!(executable.name.as_str(), "rustfmt" | "cargo-fmt")
                    || matches!(executable.path.as_str(), "bin/rustfmt" | "bin/cargo-fmt")))
            {
                super::rustfmt_probes::validate(recipe, platform, executable, file, key)?;
            } else {
                super::probes::validate_recipe_probe(
                    &recipe.version,
                    &recipe.backend,
                    platform.platform,
                    executable,
                    file,
                    key,
                )?;
            }
            validate_core_layout(recipe, executable, file, key)?;
        }
    }
    Ok(())
}

fn validate_core_layout(
    recipe: &QualifiedToolRecipe,
    executable: &super::QualifiedToolExecutableRecipe,
    file: &str,
    key: &str,
) -> Result<(), ContractError> {
    let valid = match &recipe.backend {
        QualifiedToolBackend::Core { tool } if tool == "zig" => {
            executable.name == "zig"
                && executable.path == "zig"
                && matches!(
                    &executable.probe,
                    super::QualifiedToolProbe::VersionSubcommand { .. }
                )
        }
        QualifiedToolBackend::Core { tool } if tool == "python" => {
            let mut version = recipe.version.split('.');
            let (Some(major), Some(minor), Some(_patch)) =
                (version.next(), version.next(), version.next())
            else {
                return Err(ContractError::config(
                    file,
                    key,
                    "invalid_python_tool_identity",
                ));
            };
            let path = format!("bin/python{major}.{minor}");
            executable.name == "python"
                && executable.path == path
                && matches!(&executable.probe, super::QualifiedToolProbe::Version { .. })
        }
        _ => true,
    };
    if !valid {
        return Err(ContractError::config(
            file,
            key,
            "invalid_core_tool_executable_layout",
        ));
    }
    Ok(())
}

fn validate_options(
    recipe: &QualifiedToolRecipe,
    file: &str,
    key: &str,
) -> Result<(), ContractError> {
    let bad = |problem| ContractError::config(file, key, problem);
    match (&recipe.backend, &recipe.options) {
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
            if matches!(name.as_str(), "node" | "bun" | "zig" | "python") => {}
        (QualifiedToolBackend::Aqua { package }, QualifiedToolOptions::Default)
            if super::sources::safe_package(package, true) => {}
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
                    if super::validation::sha256(source_lock_sha256) => {}
                QualifiedCargoInstallation::Prebuilt { repository }
                    if *default_features
                        && features.is_empty()
                        && super::sources::safe_package(repository, false) => {}
                _ => return Err(bad("invalid_qualified_cargo_installation")),
            }
        }
        _ => return Err(bad("unsupported_qualified_backend_options")),
    }
    Ok(())
}

pub(super) fn validate_registry(
    recipes: &[QualifiedToolRecipe],
    file: &str,
) -> Result<(), ContractError> {
    let bad = |problem| ContractError::config(file, "qualified_tools", problem);
    if recipes.len() > 128 || !recipes.windows(2).all(|pair| pair[0].id < pair[1].id) {
        return Err(bad("qualified_tools_must_be_sorted_unique"));
    }
    let by_id: BTreeMap<_, _> = recipes
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect();
    for (index, recipe) in recipes.iter().enumerate() {
        validate_recipe(recipe, file, &format!("qualified_tools[{index}]"))?;
        if recipe
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
    validate_prerequisites(recipes, &by_id, file)
}

fn validate_prerequisites(
    recipes: &[QualifiedToolRecipe],
    by_id: &BTreeMap<&str, &QualifiedToolRecipe>,
    file: &str,
) -> Result<(), ContractError> {
    for recipe in recipes {
        let dependencies: Vec<_> = recipe
            .depends_on
            .iter()
            .filter_map(|id| by_id.get(id.as_str()).copied())
            .collect();
        let rust_self =
            matches!(&recipe.backend, QualifiedToolBackend::Core { tool } if tool == "rust");
        let compilers = dependencies
            .iter()
            .filter(|dependency| {
                matches!(&dependency.backend, QualifiedToolBackend::Core { tool } if tool == "rust")
            })
            .count();
        if requires_compiler(recipe) && !rust_self && compilers != 1 {
            return Err(ContractError::config(
                file,
                "qualified_tools",
                "qualified_tool_requires_one_direct_rust_dependency",
            ));
        }
        for platform in &recipe.platforms {
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

pub(super) fn requires_compiler(recipe: &QualifiedToolRecipe) -> bool {
    matches!(&recipe.backend, QualifiedToolBackend::Core { tool } if tool == "rust")
        || matches!(
            &recipe.options,
            QualifiedToolOptions::Cargo {
                installation: QualifiedCargoInstallation::Source { .. },
                ..
            }
        )
        || recipe.platforms.iter().any(|platform| {
            platform.executables.iter().any(|executable| {
                matches!(executable.name.as_str(), "cargo" | "rustc")
                    || matches!(
                        executable.probe,
                        super::QualifiedToolProbe::CargoNextestVersion { .. }
                            | super::QualifiedToolProbe::RustcVerbose { .. }
                    )
            })
        })
}
