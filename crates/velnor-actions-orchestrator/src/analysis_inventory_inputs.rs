//! Cargo resolution and inferred target identity, independent of source edits.

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::{canonical_json_bytes, digest_b3, normalize_posix_path};
use velnor_actions_rust::WorkspaceRecord;

use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

#[path = "analysis_inventory_membership.rs"]
mod membership;

/// Complete path inventory catches inferred targets; Cargo input bytes catch
/// dependency, feature, package, resolver and target configuration changes.
/// Ordinary source bytes belong to the planner's independent semantic closure.
pub(crate) fn resolution_inputs_digest(
    root: &Path,
    files: &[String],
    inventories: &[(String, WorkspaceRecord)],
) -> Result<String, String> {
    reject_ambient_config(root)?;
    let files: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let mut inputs = BTreeSet::new();
    inputs.extend(
        crate::toolcheck::TOOL_INPUT_PATHS
            .into_iter()
            .map(str::to_owned),
    );
    let mut inferred_paths = std::collections::BTreeMap::new();
    let membership = membership::capture(root, inventories)?;
    for path in &files {
        check_path(path)?;
        let name = path.rsplit('/').next().unwrap_or(path);
        // These are Cargo's literal supported input names, not affected-work
        // classification. Every path is still bound for inferred target changes.
        if matches!(
            name,
            "Cargo.toml" | "Cargo.lock" | "rust-toolchain" | "rust-toolchain.toml"
        ) || path.ends_with(".cargo/config")
            || path.ends_with(".cargo/config.toml")
        {
            inputs.insert((*path).to_owned());
        }
    }
    for (manifest, record) in inventories {
        check_path(manifest)?;
        inputs.insert(manifest.clone());
        for package in &record.packages {
            if package.external {
                return Err("analysis_inventory_external_package".to_owned());
            }
            check_path(&package.manifest)?;
            inputs.insert(package.manifest.clone());
            cargo_ancestors(&package.manifest, &mut inputs);
            let directory = Path::new(&package.manifest)
                .parent()
                .unwrap_or(Path::new(""));
            infer_target_paths(root, directory, &mut inferred_paths)?;
        }
        let prefix = &record.workspace_root;
        check_path_or_root(prefix)?;
        cargo_ancestors(manifest, &mut inputs);
        for name in [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain",
            "rust-toolchain.toml",
        ] {
            inputs.insert(if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            });
        }
    }
    let mut content = Vec::with_capacity(inputs.len());
    for path in inputs {
        let identity = match read_repo_file(root, &path, MAX_REPO_FILE_BYTES)
            .map_err(|err| err.to_string())?
        {
            RepoRead::Absent => None,
            RepoRead::Text(text) => Some(digest_b3(text.as_bytes())),
        };
        content.push((path, identity));
    }
    let bytes = canonical_json_bytes(&(
        files,
        membership,
        inferred_paths,
        content,
        effective_environment(root)?,
    ))
    .map_err(|err| err.to_string())?;
    Ok(digest_b3(&bytes))
}

fn effective_environment(root: &Path) -> Result<Vec<(String, String)>, String> {
    let parent: Vec<_> = std::env::vars_os().collect();
    let command = velnor_actions_mise::MetadataDiscovery::new(root.join("Cargo.toml"))
        .map_err(|err| err.to_string())?
        .command(&velnor_actions_mise::ToolCatalog::pinned())
        .map_err(|err| err.to_string())?
        .with_cwd(root.to_path_buf());
    Ok(command
        .spawn_env(&parent)
        .into_iter()
        .filter(|(key, _)| resolution_environment(&key.to_string_lossy()))
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                digest_b3(value.as_encoded_bytes()),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>()
        .into_iter()
        .collect())
}

fn check_path(path: &str) -> Result<(), String> {
    let normalized = normalize_posix_path(path).map_err(|err| err.to_string())?;
    if normalized != path || path.is_empty() {
        return Err(format!("analysis_inventory_noncanonical_path:{path}"));
    }
    Ok(())
}

fn check_path_or_root(path: &str) -> Result<(), String> {
    if path.is_empty() {
        Ok(())
    } else {
        check_path(path)
    }
}

fn resolution_environment(key: &str) -> bool {
    key.starts_with("CARGO_") && !matches!(key, "CARGO_HOME" | "CARGO_TARGET_DIR")
        || matches!(
            key,
            "RUSTFLAGS" | "RUSTDOCFLAGS" | "RUSTC" | "RUSTC_WRAPPER" | "RUSTC_WORKSPACE_WRAPPER"
        )
}

fn reject_ambient_config(root: &Path) -> Result<(), String> {
    for ancestor in root.ancestors().skip(1) {
        for name in ["Cargo.toml", ".cargo/config", ".cargo/config.toml"] {
            if ancestor
                .join(name)
                .try_exists()
                .map_err(|err| err.to_string())?
            {
                return Err("analysis_inventory_ambient_cargo_config".to_owned());
            }
        }
    }
    let cargo_home = std::env::var_os("MISE_CARGO_HOME")
        .or_else(|| std::env::var_os("CARGO_HOME"))
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".cargo"))
        });
    if let Some(home) = cargo_home {
        for name in ["config", "config.toml"] {
            if home
                .join(name)
                .try_exists()
                .map_err(|err| err.to_string())?
            {
                return Err("analysis_inventory_ambient_cargo_config".to_owned());
            }
        }
    }
    Ok(())
}

fn cargo_ancestors(manifest: &str, inputs: &mut BTreeSet<String>) {
    for directory in Path::new(manifest)
        .parent()
        .into_iter()
        .flat_map(Path::ancestors)
    {
        for name in [
            "Cargo.toml",
            "Cargo.lock",
            ".cargo/config",
            ".cargo/config.toml",
            "rust-toolchain",
            "rust-toolchain.toml",
        ] {
            inputs.insert(directory.join(name).to_string_lossy().into_owned());
        }
    }
}

/// Cargo's actual automatic target roots, including ignored and excluded paths.
fn infer_target_paths(
    root: &Path,
    directory: &Path,
    paths: &mut std::collections::BTreeMap<String, bool>,
) -> Result<(), String> {
    let mut pending: Vec<_> = ["src", "examples", "tests", "benches", "build.rs"]
        .into_iter()
        .map(|name| directory.join(name))
        .collect();
    while let Some(relative) = pending.pop() {
        let full = root.join(&relative);
        let metadata = match std::fs::symlink_metadata(&full) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err.to_string()),
        };
        let path = relative
            .to_str()
            .ok_or("analysis_inventory_path_non_utf8")?;
        check_path(path)?;
        if metadata.is_symlink() {
            return Err(format!("analysis_inventory_target_symlink:{path}"));
        }
        paths.insert(path.to_owned(), metadata.is_dir());
        if paths.len() > 20000 {
            return Err("analysis_inventory_target_path_limit".to_owned());
        }
        if metadata.is_dir() {
            for entry in std::fs::read_dir(full).map_err(|err| err.to_string())? {
                pending.push(relative.join(entry.map_err(|err| err.to_string())?.file_name()));
            }
        } else if !metadata.is_file() {
            return Err("analysis_inventory_target_kind".to_owned());
        }
    }
    Ok(())
}
