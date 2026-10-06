//! Move verified Rust distribution component payloads into one owned sysroot.
use crate::{OrchestratorError, internal::internal};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use velnor_actions_contract_config::config::{
    CheckPlatform, QualifiedTool, QualifiedToolBackend, QualifiedToolOptions,
};
use velnor_actions_mise::CheckDeadline;

pub(crate) fn normalize_rust_payload(
    tool: &QualifiedTool,
    platform: CheckPlatform,
    roots: &[PathBuf],
    prefix: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    if !prefix.is_absolute() || roots.is_empty() {
        return Err(internal("rust_layout_requires_owned_absolute_paths"));
    }
    if !matches!(&tool.backend, QualifiedToolBackend::Core { tool } if tool == "rust")
        || !tool.platforms.iter().any(|p| p.platform == platform)
        || roots
            .iter()
            .any(|root| root.starts_with(prefix) || prefix.starts_with(root))
    {
        return Err(internal("invalid_rust_layout_binding"));
    }
    let selected = selected_components(tool, platform, deadline)?;
    let inventory = component_inventory(roots, deadline)?;
    for component in &selected {
        check_deadline(deadline)?;
        if !inventory.contains_key(component) {
            return Err(internal(&format!("rust_component_missing:{component}")));
        }
    }
    ensure_directory(prefix, deadline)?;
    if fs::read_dir(prefix)
        .map_err(|e| io(prefix, e))?
        .next()
        .is_some()
    {
        return Err(internal("rust_prefix_must_be_empty"));
    }
    for component in selected {
        check_deadline(deadline)?;
        let root = inventory
            .get(&component)
            .ok_or_else(|| internal("rust_component_inventory_changed"))?;
        merge_component(&root.join(component), prefix, deadline)?;
    }
    let canonical_prefix = prefix.canonicalize().map_err(|e| io(prefix, e))?;
    verify_links(prefix, &canonical_prefix, deadline)
}

fn selected_components(
    tool: &QualifiedTool,
    platform: CheckPlatform,
    deadline: CheckDeadline,
) -> Result<BTreeSet<String>, OrchestratorError> {
    let QualifiedToolOptions::Rust {
        components,
        targets,
    } = &tool.options
    else {
        return Err(internal("rust_layout_requires_rust_options"));
    };
    let mut selected = BTreeSet::from([
        "cargo".to_owned(),
        "rustc".to_owned(),
        format!("rust-std-{}", platform.target()),
    ]);
    let mut declared = BTreeSet::new();
    for component in components {
        check_deadline(deadline)?;
        let name = match component.as_str() {
            "rust-std" => format!("rust-std-{}", platform.target()),
            "rustfmt" => "rustfmt-preview".to_owned(),
            "clippy" => "clippy-preview".to_owned(),
            "rust-analyzer" => "rust-analyzer-preview".to_owned(),
            "llvm-tools" => "llvm-tools-preview".to_owned(),
            name => name.to_owned(),
        };
        if !safe_name(&name) || !declared.insert(name.clone()) {
            return Err(internal("rust_component_alias_ambiguity"));
        }
        selected.insert(name);
    }
    for target in targets {
        check_deadline(deadline)?;
        selected.insert(format!("rust-std-{target}"));
    }
    Ok(selected)
}

fn component_inventory(
    roots: &[PathBuf],
    deadline: CheckDeadline,
) -> Result<BTreeMap<String, PathBuf>, OrchestratorError> {
    let mut inventory = BTreeMap::new();
    for root in roots {
        check_deadline(deadline)?;
        let root = package_root(root, deadline)?;
        let path = root.join("components");
        let contents = read_manifest(&path, deadline)?;
        let mut names = BTreeSet::new();
        for name in contents.lines() {
            check_deadline(deadline)?;
            if !safe_name(name) || !names.insert(name.to_owned()) {
                return Err(internal("invalid_rust_components_manifest"));
            }
            if inventory.insert(name.to_owned(), root.clone()).is_some() {
                return Err(internal(&format!("ambiguous_rust_component:{name}")));
            }
        }
        if names.is_empty() {
            return Err(internal("empty_rust_components_manifest"));
        }
    }
    Ok(inventory)
}

fn package_root(root: &Path, deadline: CheckDeadline) -> Result<PathBuf, OrchestratorError> {
    check_deadline(deadline)?;
    if !root.is_absolute() {
        return Err(internal("rust_archive_root_not_absolute"));
    }
    require_directory(root)?;
    if root.join("components").exists() {
        return Ok(root.to_owned());
    }
    let mut packages = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| io(root, e))? {
        check_deadline(deadline)?;
        let path = entry.map_err(|e| io(root, e))?.path();
        if fs::symlink_metadata(&path)
            .map_err(|e| io(&path, e))?
            .is_dir()
            && path.join("components").exists()
        {
            packages.push(path);
        }
    }
    if packages.len() != 1 {
        return Err(internal("rust_archive_wrapper_ambiguous"));
    }
    packages
        .pop()
        .ok_or_else(|| internal("rust_archive_wrapper_missing"))
}

fn merge_component(
    root: &Path,
    prefix: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    require_directory(root)?;
    let manifest = read_manifest(&root.join("manifest.in"), deadline)?;
    let mut entries = BTreeSet::new();
    for line in manifest.lines() {
        check_deadline(deadline)?;
        let (kind, relative) = line
            .split_once(':')
            .ok_or_else(|| internal("invalid_rust_payload_manifest"))?;
        if !matches!(kind, "file" | "dir")
            || !safe_relative(relative)
            || !entries.insert(relative.to_owned())
        {
            return Err(internal("invalid_rust_payload_manifest"));
        }
        let source = root.join(relative);
        validate_payload_ancestors(root, relative, deadline)?;
        let metadata = fs::symlink_metadata(&source).map_err(|e| io(&source, e))?;
        if (kind == "dir" && !metadata.is_dir()) || (kind == "file" && metadata.is_dir()) {
            return Err(internal("rust_payload_manifest_type_mismatch"));
        }
    }
    if entries.is_empty() {
        return Err(internal("empty_rust_payload_manifest"));
    }
    for relative in entries {
        check_deadline(deadline)?;
        move_payload(
            &root.join(&relative),
            &prefix.join(relative),
            prefix,
            deadline,
        )?;
    }
    Ok(())
}

fn validate_payload_ancestors(
    root: &Path,
    relative: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let mut current = root.to_owned();
    let parts: Vec<_> = Path::new(relative).components().collect();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        check_deadline(deadline)?;
        current.push(part.as_os_str());
        require_directory(&current)?;
    }
    Ok(())
}

fn move_payload(
    source: &Path,
    destination: &Path,
    prefix: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    let metadata = fs::symlink_metadata(source).map_err(|e| io(source, e))?;
    let parent = destination
        .parent()
        .ok_or_else(|| internal("rust_payload_parent_missing"))?;
    ensure_payload_parent(parent, prefix, deadline)?;
    match fs::symlink_metadata(destination) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::rename(source, destination).map_err(|e| io(destination, e))
        }
        Ok(existing) if existing.is_dir() && metadata.is_dir() => {
            for entry in fs::read_dir(source).map_err(|e| io(source, e))? {
                check_deadline(deadline)?;
                let entry = entry.map_err(|e| io(source, e))?;
                move_payload(
                    &entry.path(),
                    &destination.join(entry.file_name()),
                    prefix,
                    deadline,
                )?;
            }
            fs::remove_dir(source).map_err(|e| io(source, e))
        }
        Ok(_) => Err(internal("rust_component_payload_collision")),
        Err(error) => Err(io(destination, error)),
    }
}

fn ensure_payload_parent(
    parent: &Path,
    prefix: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let relative = parent
        .strip_prefix(prefix)
        .map_err(|_| internal("rust_payload_parent_escapes_prefix"))?;
    let mut current = prefix.to_owned();
    require_directory(&current)?;
    for component in relative.components() {
        check_deadline(deadline)?;
        if !matches!(component, Component::Normal(_)) {
            return Err(internal("invalid_rust_payload_parent"));
        }
        current.push(component);
        ensure_directory(&current, deadline)?;
    }
    Ok(())
}

fn ensure_directory(path: &Path, deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(internal("rust_payload_directory_collision")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .ok_or_else(|| internal("rust_payload_parent_missing"))?;
            ensure_directory(parent, deadline)?;
            fs::create_dir(path).map_err(|e| io(path, e))
        }
        Err(error) => Err(io(path, error)),
    }
}

fn verify_links(
    path: &Path,
    prefix: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    for entry in fs::read_dir(path).map_err(|e| io(path, e))? {
        check_deadline(deadline)?;
        let path = entry.map_err(|e| io(path, e))?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|e| io(&path, e))?;
        if metadata.is_symlink() {
            if fs::read_link(&path)
                .map_err(|e| io(&path, e))?
                .is_absolute()
                || !path
                    .canonicalize()
                    .map_err(|e| io(&path, e))?
                    .starts_with(prefix)
            {
                return Err(internal("rust_payload_link_escapes_prefix"));
            }
        } else if metadata.is_dir() {
            verify_links(&path, prefix, deadline)?;
        } else if !metadata.is_file() {
            return Err(internal("rust_payload_special_file"));
        }
    }
    Ok(())
}

fn read_manifest(path: &Path, deadline: CheckDeadline) -> Result<String, OrchestratorError> {
    check_deadline(deadline)?;
    let metadata = fs::symlink_metadata(path).map_err(|e| io(path, e))?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err(internal("rust_manifest_not_bounded_regular_file"));
    }
    let content = fs::read_to_string(path).map_err(|e| io(path, e))?;
    check_deadline(deadline)?;
    Ok(content)
}

fn require_directory(path: &Path) -> Result<(), OrchestratorError> {
    if !fs::symlink_metadata(path)
        .map_err(|e| io(path, e))?
        .is_dir()
    {
        return Err(internal("rust_component_not_directory"));
    }
    Ok(())
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
        && path
            .bytes()
            .all(|b| b.is_ascii_graphic() && b != b'\\' && b != b':')
        && Path::new(path)
            .components()
            .all(|p| matches!(p, Component::Normal(_)))
}

fn io(path: &Path, error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

fn check_deadline(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|error| internal(&error.to_string()))
}
