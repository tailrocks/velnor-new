// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use crate::process::run_bounded;

const METADATA_CAP: usize = 512 * 1024;
const METADATA_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) fn print_workspace_members(root: &Path, libraries_only: bool) -> i32 {
    match cargo_members(root, libraries_only) {
        Ok(members) => {
            println!("{}", members.join(" "));
            0
        }
        Err(error) => {
            eprintln!("repository policy: {error}");
            1
        }
    }
}

pub(crate) fn print_test_members(root: &Path, manifest_path: Option<&Path>) -> i32 {
    match cargo_test_members(root, manifest_path) {
        Ok(members) => {
            println!("{}", members.join(" "));
            0
        }
        Err(error) => {
            eprintln!("repository policy: {error}");
            1
        }
    }
}

fn cargo_members(root: &Path, libraries_only: bool) -> Result<Vec<String>, String> {
    let stdout = cargo_metadata(root, None)?;
    let metadata: Value = serde_json::from_slice(&stdout)
        .map_err(|error| format!("cargo metadata output was invalid JSON ({error})"))?;
    let packages = metadata["packages"]
        .as_array()
        .ok_or_else(|| "cargo metadata had no packages array".to_owned())?;
    let mut names = packages
        .iter()
        .filter(|package| !libraries_only || has_library_target(package))
        .filter_map(|package| package["name"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    if names.is_empty() {
        return Err("cargo metadata returned no matching package names".to_owned());
    }
    Ok(names)
}

fn cargo_test_members(root: &Path, manifest_path: Option<&Path>) -> Result<Vec<String>, String> {
    let stdout = cargo_metadata(root, manifest_path)?;
    let metadata: Value = serde_json::from_slice(&stdout)
        .map_err(|error| format!("cargo metadata output was invalid JSON ({error})"))?;
    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| "cargo metadata had no packages array".to_owned())?;
    let workspace_members = metadata
        .get("workspace_members")
        .and_then(Value::as_array)
        .ok_or_else(|| "cargo metadata had no workspace_members array".to_owned())?;
    let mut workspace_ids = BTreeSet::new();
    for (index, member) in workspace_members.iter().enumerate() {
        let id = member
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| format!("cargo metadata workspace member {index} had no package ID"))?;
        if !workspace_ids.insert(id.to_owned()) {
            return Err(format!("cargo metadata repeated workspace member {id}"));
        }
    }
    let mut package_ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    for (package_index, package) in packages.iter().enumerate() {
        let id = package
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| format!("cargo metadata package {package_index} had no package ID"))?;
        let name = package
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| {
                !name.is_empty()
                    && !name
                        .chars()
                        .any(|character| character.is_whitespace() || character.is_control())
            })
            .ok_or_else(|| format!("cargo metadata package {id} had no valid package name"))?;
        let targets = package
            .get("targets")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("cargo metadata package {id} had no targets array"))?;
        if !package_ids.insert(id.to_owned()) {
            return Err(format!("cargo metadata repeated package ID {id}"));
        }
        let mut has_test_target = false;
        for (target_index, target) in targets.iter().enumerate() {
            let test = target.get("test").and_then(Value::as_bool).ok_or_else(|| {
                format!("cargo metadata target {target_index} for package {id} had no test boolean")
            })?;
            has_test_target |= test;
        }
        if workspace_ids.contains(id) && has_test_target {
            names.insert(name.to_owned());
        }
    }
    if let Some(id) = workspace_ids.difference(&package_ids).next() {
        return Err(format!(
            "cargo metadata workspace member {id} was missing from packages"
        ));
    }
    Ok(names.into_iter().collect())
}

fn cargo_metadata(root: &Path, manifest_path: Option<&Path>) -> Result<Vec<u8>, String> {
    let manifest_path = manifest_path
        .map(|path| resolve_manifest_path(root, path))
        .transpose()?;
    let mut command = Command::new("cargo");
    command.args([
        "metadata",
        "--locked",
        "--no-deps",
        "--format-version",
        "1",
        "--offline",
    ]);
    if let Some(path) = manifest_path {
        command.arg("--manifest-path").arg(path);
    }
    let output = run_bounded(command.current_dir(root), METADATA_CAP, METADATA_TIMEOUT)
        .map_err(|error| format!("cargo metadata failed ({error})"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output.stdout)
}

fn resolve_manifest_path(root: &Path, manifest_path: &Path) -> Result<PathBuf, String> {
    if manifest_path.is_absolute()
        || manifest_path.file_name() != Some(OsStr::new("Cargo.toml"))
        || manifest_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(
            "test-members manifest path must be a repository-relative Cargo.toml path".to_owned(),
        );
    }
    let root = root
        .canonicalize()
        .map_err(|error| format!("repository root could not be resolved ({error})"))?;
    let manifest = root
        .join(manifest_path)
        .canonicalize()
        .map_err(|error| format!("test-members manifest path could not be resolved ({error})"))?;
    if !manifest.starts_with(&root) || !manifest.is_file() {
        return Err(
            "test-members manifest path must resolve to a Cargo.toml inside the repository"
                .to_owned(),
        );
    }
    Ok(manifest)
}

fn has_library_target(package: &Value) -> bool {
    package["targets"].as_array().is_some_and(|targets| {
        targets.iter().any(|target| {
            target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("lib")))
        })
    })
}
