//! Task-local Rust inputs, with explicit limits on static completeness proof.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use velnor_actions_contract::{ProposedTask, Provenance, canonical_json_bytes, digest_b3};

/// Checkout inventory whose collection and content checks succeeded together.
#[derive(Debug, Clone)]
pub struct SemanticInventory {
    /// Sorted repository-relative tracked and nonignored paths.
    pub paths: Vec<String>,
    /// Collection integrity; its repository-wide digest is not a task input.
    pub provenance: Provenance,
}

/// Resolve a task's local dependency inputs without launching Cargo or Git.
#[must_use]
pub fn resolve(root: &Path, task: &ProposedTask, inventory: &SemanticInventory) -> Provenance {
    if !matches!(inventory.provenance, Provenance::Known { .. }) {
        return Provenance::Unknown {
            reason: "checkout_inventory_unverified".to_owned(),
        };
    }
    match inputs(root, task, inventory)
        .and_then(|files| canonical_json_bytes(&files).map_err(|err| err.to_string()))
    {
        Ok(bytes) => Provenance::Known {
            digest: digest_b3(&bytes),
        },
        Err(reason) => Provenance::Unknown { reason },
    }
}

/// Manifest and source bytes for the complete reachable local package set.
fn inputs(
    root: &Path,
    task: &ProposedTask,
    inventory: &SemanticInventory,
) -> Result<BTreeMap<String, String>, String> {
    let mut files = BTreeMap::new();
    let mut pending = vec![task.identity.unit_path.clone()];
    let mut visited = BTreeSet::new();
    while let Some(manifest) = pending.pop() {
        if !visited.insert(manifest.clone()) {
            continue;
        }
        let value = read_manifest(root, &manifest, &mut files)?;
        let workspace = workspace_manifest(root, &manifest, &mut files)?;
        if workspace.as_ref().is_some_and(|(_, value)| {
            value.get("patch").is_some() || value.get("replace").is_some()
        }) {
            return Err("workspace_dependency_override_unresolved".to_owned());
        }
        validate_package(root, &manifest, &value)?;
        validate_target_paths(root, &manifest, &value, inventory, &mut files)?;
        dependencies(&manifest, &value, workspace.as_ref(), &mut pending)?;
        package_files(root, &manifest, task, inventory, &mut files)?;
    }
    for path in &task.identity.declared_inputs {
        bind_file(root, path, &mut files)?;
    }
    Ok(files)
}

/// Read a manifest, preserving its exact bytes in the closure.
fn read_manifest(
    root: &Path,
    path: &str,
    files: &mut BTreeMap<String, String>,
) -> Result<toml::Value, String> {
    let bytes = bind_file(root, path, files)?;
    let text = std::str::from_utf8(&bytes).map_err(|err| err.to_string())?;
    toml::from_str(text).map_err(|err| format!("invalid_manifest:{path}:{err}"))
}

/// Find the nearest ancestor workspace, including all inherited settings.
fn workspace_manifest(
    root: &Path,
    manifest: &str,
    files: &mut BTreeMap<String, String>,
) -> Result<Option<(String, toml::Value)>, String> {
    let mut dir = Path::new(manifest).parent();
    while let Some(parent) = dir {
        let path = parent.join("Cargo.toml");
        let path = path.to_string_lossy().replace('\\', "/");
        match std::fs::symlink_metadata(root.join(&path)) {
            Ok(_) => {
                let value = read_manifest(root, &path, files)?;
                if value.get("workspace").is_some() {
                    return Ok(Some((path, value)));
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(format!("workspace_input_unreadable:{path}:{err}")),
        }
        dir = parent.parent();
    }
    Ok(None)
}

/// Build scripts and procedural macros lack static input-completeness evidence.
fn validate_package(root: &Path, manifest: &str, value: &toml::Value) -> Result<(), String> {
    let package = value.get("package");
    let build = package.and_then(|value| value.get("build"));
    let default_build = Path::new(manifest)
        .parent()
        .unwrap_or(Path::new(""))
        .join("build.rs");
    if build.is_some_and(|value| value.as_bool() != Some(false))
        || build.is_none() && root.join(default_build).exists()
    {
        return Err(format!("unproven_build_script_inputs:{manifest}"));
    }
    if value
        .get("lib")
        .and_then(|value| value.get("proc-macro"))
        .and_then(toml::Value::as_bool)
        == Some(true)
    {
        return Err(format!("unproven_proc_macro_inputs:{manifest}"));
    }
    if package.and_then(|value| value.get("workspace")).is_some() {
        return Err(format!("explicit_workspace_unresolved:{manifest}"));
    }
    Ok(())
}

/// Custom target paths must be inventoried and checked as Rust regardless of extension.
fn validate_target_paths(
    root: &Path,
    manifest: &str,
    value: &toml::Value,
    inventory: &SemanticInventory,
    files: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    for key in ["lib", "bin", "example", "test", "bench"] {
        let Some(targets) = value.get(key) else {
            continue;
        };
        let entries = targets
            .as_array()
            .map_or_else(|| vec![targets], |entries| entries.iter().collect());
        for target in entries {
            let Some(path) = target.get("path").and_then(toml::Value::as_str) else {
                continue;
            };
            let path = relative_path(manifest, path)?;
            let package_dir = Path::new(manifest).parent().unwrap_or(Path::new(""));
            if !Path::new(&path).starts_with(package_dir) {
                return Err(format!("external_target_source:{path}"));
            }
            if !inventory.paths.contains(&path) {
                return Err(format!("unverified_target_source:{path}"));
            }
            let bytes = bind_file(root, &path, files)?;
            source_completeness(&path, &bytes)?;
            if Path::new(&path)
                .components()
                .any(|part| part.as_os_str() == "target")
            {
                return Err(format!("generated_target_source:{path}"));
            }
        }
    }
    Ok(())
}

/// Include every normal/build/dev/optional/target-specific local dependency.
fn dependencies(
    manifest: &str,
    value: &toml::Value,
    workspace: Option<&(String, toml::Value)>,
    pending: &mut Vec<String>,
) -> Result<(), String> {
    for key in ["dependencies", "build-dependencies", "dev-dependencies"] {
        if let Some(table) = value.get(key).and_then(toml::Value::as_table) {
            for (name, dependency) in table {
                let (base, dependency) =
                    inherited_dependency(manifest, name, dependency, workspace)?;
                let path = dependency
                    .get("path")
                    .and_then(toml::Value::as_str)
                    .ok_or_else(|| format!("opaque_dependency_inputs:{manifest}:{name}"))?;
                pending.push(relative_path(base, &format!("{path}/Cargo.toml"))?);
            }
        }
    }
    if let Some(targets) = value.get("target").and_then(toml::Value::as_table) {
        for target in targets.values() {
            dependencies(manifest, target, workspace, pending)?;
        }
    }
    if value.get("patch").is_some() || value.get("replace").is_some() {
        return Err(format!("dependency_override_unresolved:{manifest}"));
    }
    Ok(())
}

/// Workspace dependency paths are relative to the workspace manifest.
fn inherited_dependency<'a>(
    manifest: &'a str,
    name: &str,
    dependency: &'a toml::Value,
    workspace: Option<&'a (String, toml::Value)>,
) -> Result<(&'a str, &'a toml::Value), String> {
    if dependency.get("workspace").and_then(toml::Value::as_bool) != Some(true) {
        return Ok((manifest, dependency));
    }
    let (path, value) =
        workspace.ok_or_else(|| format!("workspace_dependency_unresolved:{name}"))?;
    let inherited = value
        .get("workspace")
        .and_then(|value| value.get("dependencies"))
        .and_then(|value| value.get(name))
        .ok_or_else(|| format!("workspace_dependency_missing:{name}"))?;
    Ok((path, inherited))
}

/// Normalize local paths lexically, rejecting escape from the checkout.
fn relative_path(manifest: &str, path: &str) -> Result<String, String> {
    let mut normalized = PathBuf::from(Path::new(manifest).parent().unwrap_or(Path::new("")));
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir if normalized.pop() => {}
            _ => return Err(format!("outside_checkout:{path}")),
        }
    }
    Ok(normalized.to_string_lossy().replace('\\', "/"))
}

/// Bind package sources and task-relevant assets from verified inventory.
fn package_files(
    root: &Path,
    manifest: &str,
    task: &ProposedTask,
    inventory: &SemanticInventory,
    files: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    let dir = Path::new(manifest).parent().unwrap_or(Path::new(""));
    let docs = matches!(task.task_kind.as_str(), "doc" | "doctest");
    for path in &inventory.paths {
        if !Path::new(path).starts_with(dir) {
            continue;
        }
        let markdown = Path::new(path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
        if markdown && !docs {
            continue;
        }
        let bytes = bind_file(root, path, files)?;
        if Path::new(path).extension().is_some_and(|ext| ext == "rs") {
            source_completeness(path, &bytes)?;
        }
    }
    // Inventory omits ignored generated sources; their presence cannot be guessed away.
    ignored_sources(root, dir, inventory)?;
    Ok(())
}

/// Any source excluded from the inventory makes exact semantic proof unknown.
fn ignored_sources(root: &Path, dir: &Path, inventory: &SemanticInventory) -> Result<(), String> {
    let mut pending = vec![root.join(dir)];
    let mut visited = 0_u32;
    while let Some(dir) = pending.pop() {
        visited += 1;
        if visited > 2000 {
            return Err("semantic_source_walk_limit".to_owned());
        }
        for entry in std::fs::read_dir(dir).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            let kind = entry.file_type().map_err(|err| err.to_string())?;
            if kind.is_symlink() {
                return Err("semantic_source_symlink".to_owned());
            }
            if kind.is_dir() && entry.file_name() != ".git" {
                pending.push(entry.path());
            } else if entry.path().extension().is_some_and(|ext| ext == "rs") {
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .map_err(|err| err.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/");
                if !inventory.paths.contains(&relative) {
                    return Err(format!("ignored_source:{relative}"));
                }
            }
        }
    }
    Ok(())
}

/// Reject language constructs whose reads cannot be proven with this parser.
fn source_completeness(path: &str, bytes: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(bytes).map_err(|err| err.to_string())?;
    let words: BTreeSet<&str> = text
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .collect();
    for unknown in [
        "std",
        "extern",
        "unsafe",
        "use",
        "include",
        "include_str",
        "include_bytes",
        "env",
        "option_env",
        "derive",
        "path",
        "macro_rules",
    ] {
        if words.contains(unknown) {
            return Err(format!("unproven_source_inputs:{path}:{unknown}"));
        }
    }
    if text.contains('!') {
        return Err(format!("unproven_macro_inputs:{path}"));
    }
    Ok(())
}

/// Read a regular file through symlink-free repository-relative components.
fn bind_file(
    root: &Path,
    path: &str,
    files: &mut BTreeMap<String, String>,
) -> Result<Vec<u8>, String> {
    let normalized =
        crate::identity::normalize_identity_path(path).map_err(|err| err.to_string())?;
    let mut full = root.to_path_buf();
    for component in Path::new(&normalized).components() {
        full.push(component);
        let metadata = std::fs::symlink_metadata(&full)
            .map_err(|err| format!("semantic_input:{path}:{err}"))?;
        if metadata.file_type().is_symlink() {
            return Err(format!("semantic_symlink:{path}"));
        }
    }
    let metadata = std::fs::metadata(root.join(&normalized)).map_err(|err| err.to_string())?;
    if !metadata.is_file() {
        return Err(format!("semantic_nonregular_input:{path}"));
    }
    let bytes = std::fs::read(full).map_err(|err| format!("semantic_input:{path}:{err}"))?;
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777
    };
    #[cfg(not(unix))]
    let permissions = u32::from(metadata.permissions().readonly());
    let identity =
        canonical_json_bytes(&(digest_b3(&bytes), permissions)).map_err(|err| err.to_string())?;
    files.insert(normalized, digest_b3(&identity));
    Ok(bytes)
}

#[cfg(test)]
#[path = "semantic_inputs_tests.rs"]
mod tests;
