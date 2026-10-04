//! Shared dependency and confined path policy.
use crate::OrchestratorError;
use std::path::{Component, Path};
pub(super) fn validate_dependency(dependency: &toml::Table) -> Result<(), OrchestratorError> {
    for (key, value) in dependency {
        let valid = match key.as_str() {
            "version" | "path" | "package" => value.as_str().is_some_and(|value| !value.is_empty()),
            "registry" => value.as_str() == Some("crates-io"),
            "workspace" => value.as_bool() == Some(true),
            "optional" | "default-features" | "public" => value.as_bool().is_some(),
            "features" => value
                .as_array()
                .is_some_and(|values| values.iter().all(|value| value.as_str().is_some())),
            _ => false,
        };
        if !valid {
            return Err(unqualified("unknown_or_private_dependency"));
        }
    }
    if dependency.contains_key("workspace") {
        if dependency.keys().any(|key| {
            !matches!(
                key.as_str(),
                "workspace" | "features" | "optional" | "default-features"
            )
        }) {
            return Err(unqualified("mixed_workspace_dependency"));
        }
    } else if !dependency.contains_key("path") && !dependency.contains_key("version") {
        return Err(unqualified("dependency_source_missing"));
    }
    Ok(())
}

pub(super) fn dependency_manifest(
    manifest: &str,
    dependency: &str,
) -> Result<String, OrchestratorError> {
    if Path::new(dependency).is_absolute() || dependency.contains('\\') {
        return Err(unqualified("absolute_dependency_path"));
    }
    let base = Path::new(manifest)
        .parent()
        .ok_or_else(|| unqualified("manifest_parent"))?;
    let mut parts = Vec::new();
    for component in base.join(dependency).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir if parts.pop().is_some() => {}
            _ => return Err(unqualified("dependency_path_escape")),
        }
    }
    parts.push("Cargo.toml".to_owned());
    Ok(parts.join("/"))
}

pub(super) fn relative(path: &str, empty: bool) -> Result<(), OrchestratorError> {
    if (path.is_empty() && !empty)
        || path.contains('\\')
        || path.contains('\0')
        || (!path.is_empty() && path.split('/').any(|part| matches!(part, "" | "." | "..")))
        || Path::new(path)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(unqualified("invalid_relative_path"));
    }
    Ok(())
}

pub(super) fn as_table(value: &toml::Value) -> Result<&toml::Table, OrchestratorError> {
    value
        .as_table()
        .ok_or_else(|| unqualified("malformed_dependency_table"))
}

pub(super) fn unqualified(reason: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("source_manifest_unqualified:{reason}"),
    }
}
