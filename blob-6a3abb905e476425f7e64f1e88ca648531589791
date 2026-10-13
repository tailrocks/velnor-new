//! Resolve nested checks to Cargo's workspace target directory through pinned MBX.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

use super::{Outcome, PackagePlan, TestTarget, WorkspacePlan};
use crate::impl_repo_policy::{WORKSPACE_ROOTS, repo_root};

pub(super) fn cargo_target_dir_for_manifest(manifest: &Path) -> Outcome<PathBuf> {
    if let Some(configured) = std::env::var_os("CARGO_TARGET_DIR") {
        let current_dir = std::env::current_dir()?;
        return Ok(absolute_path(&current_dir, Path::new(&configured)));
    }
    let metadata = mbx_output(
        manifest,
        &["metadata", "--no-deps", "--format-version", "1", "--locked"],
    )?;
    let document: Value = serde_json::from_slice(&metadata.stdout)?;
    let root = manifest
        .parent()
        .ok_or("workspace manifest has no parent")?
        .canonicalize()?;
    metadata_target_directory(&document, &root)
}

fn metadata_target_directory(document: &Value, root: &Path) -> Outcome<PathBuf> {
    let target_directory = PathBuf::from(
        document["target_directory"]
            .as_str()
            .ok_or("metadata target directory")?,
    );
    existing_or_absolute(root, &target_directory)
}

fn pinned_mbx(parent: &[(OsString, OsString)], current_dir: &Path) -> Outcome<(PathBuf, String)> {
    let catalog = crate::impl_repo_policy::read("crates/velnor-actions-mise/src/catalog.rs")?;
    let mbx_version = crate::impl_repo_policy::quoted_value(&catalog, "MR_BOXINGTON_VERSION")?;
    let rust_version = crate::impl_repo_policy::quoted_value(&catalog, "RUST_VERSION")?;
    let executable = parent_env_value(parent, "PATH")
        .and_then(|path| mbx_path_from_path(path, current_dir))
        .ok_or("pinned MBX executable is missing from PATH")?;
    let mut probe = Command::new(&executable);
    probe
        .arg("--version")
        .env_clear()
        .envs(mbx_child_environment(parent, &isolation_environment()));
    let version = probe.output()?;
    let observed = String::from_utf8_lossy(&version.stdout);
    let expected = format!("mbx {mbx_version}");
    if !version.status.success() || observed.trim() != expected {
        return Err(format!(
            "expected MBX {expected} at {}, observed {:?} (exit {})",
            executable.display(),
            observed.trim(),
            version.status
        )
        .into());
    }
    Ok((executable, rust_version))
}

pub(super) fn mbx_output_at_target(
    manifest: &Path,
    args: &[&str],
    target_dir: &Path,
) -> Outcome<Output> {
    mbx_output_with_target(manifest, args, Some(target_dir))
}

fn mbx_output(manifest: &Path, args: &[&str]) -> Outcome<Output> {
    mbx_output_with_target(manifest, args, None)
}

fn mbx_output_with_target(
    manifest: &Path,
    args: &[&str],
    target_dir: Option<&Path>,
) -> Outcome<Output> {
    let current_dir = std::env::current_dir()?;
    let parent = std::env::vars_os().collect::<Vec<_>>();
    let manifest_dir = manifest
        .parent()
        .ok_or("workspace manifest has no parent")?;
    let (command, options) = args.split_first().ok_or("empty MBX command")?;
    let (mbx, rust_version) = pinned_mbx(&parent, &current_dir)?;
    let mut additions = isolation_environment();
    if let Some(target_dir) = target_dir {
        additions.push((
            OsString::from("CARGO_TARGET_DIR"),
            absolute_path(&current_dir, target_dir).into_os_string(),
        ));
    }
    let mut process = Command::new(mbx);
    process
        .arg(format!("+{rust_version}"))
        .arg(command)
        .arg("--manifest-path")
        .arg(manifest)
        .args(options)
        .env_clear()
        .envs(mbx_child_environment(&parent, &additions))
        .current_dir(manifest_dir);
    let output = process.output()?;
    if !output.status.success() {
        return Err(format!(
            "mbx {} failed for {}: {}{}",
            args.first().copied().unwrap_or("command"),
            manifest.display(),
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout),
        )
        .into());
    }
    Ok(output)
}

/// Mirrors the production MBX child boundary without linking this binary-only
/// integration test to the library under test.
fn mbx_child_environment(
    parent: &[(OsString, OsString)],
    additions: &[(OsString, OsString)],
) -> Vec<(OsString, OsString)> {
    let mut environment = parent
        .iter()
        .filter(|(key, _)| !mbx_parent_key_is_stripped(key))
        .cloned()
        .collect::<Vec<_>>();
    environment.extend(additions.iter().cloned());
    environment.retain(|(key, _)| !matches!(key.to_str(), Some("CARGO" | "RUSTC" | "RUSTDOC")));
    sanitize_mise_cargo_path(&mut environment, parent);
    environment
}

fn mbx_parent_key_is_stripped(key: &OsStr) -> bool {
    let Some(name) = key.to_str() else {
        return false;
    };
    name == "RUSTUP_TOOLCHAIN"
        || name == "GH_HOST"
        || name == "GH_CONFIG_DIR"
        || name.starts_with("TF_")
        || name.starts_with("TOFU_")
        || name.starts_with("CHECKPOINT_")
        || name.starts_with("CARGO_REGISTRIES_")
        || name.ends_with("_TOKEN")
        || [
            "MISE_GITHUB_TOKEN",
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "ACTIONS_RUNTIME_TOKEN",
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
            "ACTIONS_ID_TOKEN_REQUEST_URL",
            "CARGO_REGISTRY_TOKEN",
            "NPM_TOKEN",
            "NODE_AUTH_TOKEN",
        ]
        .contains(&name)
}

fn sanitize_mise_cargo_path(
    environment: &mut Vec<(OsString, OsString)>,
    parent: &[(OsString, OsString)],
) {
    let data_dir = parent_env_value(parent, "MISE_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            parent_env_value(parent, "HOME")
                .map(|home| PathBuf::from(home).join(".local/share/mise"))
        });
    let Some(data_dir) = data_dir.filter(|path| path.is_absolute()) else {
        return;
    };
    let rejected = [
        data_dir.join("command-wrappers").join("bin"),
        data_dir.join("shims"),
    ];
    let Some((_, path)) = environment.iter().rev().find(|(key, _)| key == "PATH") else {
        return;
    };
    let entries = std::env::split_paths(path)
        .filter(|entry| !rejected.iter().any(|candidate| same_path(entry, candidate)))
        .collect::<Vec<_>>();
    let Ok(path) = std::env::join_paths(entries) else {
        environment.retain(|(key, _)| key != "PATH");
        return;
    };
    environment.retain(|(key, _)| key != "PATH");
    environment.push((OsString::from("PATH"), path));
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.components().collect::<Vec<_>>();
    let right = right.components().collect::<Vec<_>>();
    left.len() == right.len()
        && left.iter().zip(right.iter()).all(|(left, right)| {
            let left = left.as_os_str().to_string_lossy();
            let right = right.as_os_str().to_string_lossy();
            if cfg!(windows) || cfg!(target_os = "macos") {
                left.eq_ignore_ascii_case(&right)
            } else {
                left == right
            }
        })
}

fn mbx_path_from_path(path: &OsStr, current_dir: &Path) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|directory| absolute_path(current_dir, &directory.join("mbx")))
        .find(|candidate| candidate.is_file())
}

fn parent_env_value<'a>(parent: &'a [(OsString, OsString)], name: &str) -> Option<&'a OsStr> {
    parent
        .iter()
        .rev()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_os_str())
}

fn isolation_environment() -> Vec<(OsString, OsString)> {
    [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
    ]
    .into_iter()
    .map(|(key, value)| (OsString::from(key), OsString::from(value)))
    .collect()
}

pub(super) fn workspace_plan(manifest: &Path) -> Outcome<WorkspacePlan> {
    let target_dir = cargo_target_dir_for_manifest(manifest)?;
    workspace_plan_at_target(manifest, &target_dir)
}

pub(super) fn workspace_plan_at_target(
    manifest: &Path,
    target_dir: &Path,
) -> Outcome<WorkspacePlan> {
    let metadata = mbx_output_at_target(
        manifest,
        &["metadata", "--no-deps", "--format-version", "1", "--locked"],
        target_dir,
    )?;
    let document: Value = serde_json::from_slice(&metadata.stdout)?;
    let root = manifest
        .parent()
        .ok_or("workspace manifest has no parent")?
        .canonicalize()?;
    let reported_target = PathBuf::from(
        document["target_directory"]
            .as_str()
            .ok_or("metadata target directory")?,
    );
    let target_directory = existing_or_absolute(&root, &reported_target)?;
    let packages = document["packages"].as_array().ok_or("metadata packages")?;
    let mut plans = Vec::new();
    for package in packages {
        let id = package["id"].as_str().ok_or("package id")?.to_owned();
        let package_manifest = PathBuf::from(
            package["manifest_path"]
                .as_str()
                .ok_or("package manifest path")?,
        );
        let package_root = package_manifest
            .parent()
            .ok_or("package manifest has no parent")?
            .canonicalize()?;
        let mut test_targets = Vec::new();
        for target in package["targets"].as_array().ok_or("package targets")? {
            if target["test"].as_bool() != Some(true) {
                continue;
            }
            let source = PathBuf::from(
                target["src_path"]
                    .as_str()
                    .ok_or("test target source path")?,
            )
            .canonicalize()?;
            let required_features = match target.get("required-features") {
                Some(features) => features
                    .as_array()
                    .ok_or("test target required features")?
                    .iter()
                    .map(|feature| {
                        feature
                            .as_str()
                            .map(str::to_owned)
                            .ok_or("test target required feature name")
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                None => Vec::new(),
            };
            test_targets.push(TestTarget {
                source,
                required_features,
            });
        }
        plans.push(PackagePlan {
            id,
            root: package_root,
            test_targets,
        });
    }
    Ok(WorkspacePlan {
        manifest: manifest.canonicalize()?,
        root,
        target_directory,
        vcs_metadata: repository_metadata_path()?,
        packages: plans,
    })
}

fn repository_metadata_path() -> Outcome<Option<PathBuf>> {
    let marker = repo_root().canonicalize()?.join(".git");
    match fs::metadata(&marker) {
        Ok(metadata) if metadata.is_dir() => Ok(Some(marker.canonicalize()?)),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn existing_or_absolute(base: &Path, path: &Path) -> Outcome<PathBuf> {
    let absolute = absolute_path(base, path);
    match absolute.canonicalize() {
        Ok(canonical) => Ok(canonical),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(absolute),
        Err(error) => Err(error.into()),
    }
}

fn absolute_path(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

pub(super) fn workspace_plans(root: &Path) -> Outcome<Vec<WorkspacePlan>> {
    WORKSPACE_ROOTS
        .iter()
        .map(|workspace| workspace_plan(&root.join(workspace).join("Cargo.toml")))
        .collect()
}

#[cfg(test)]
#[path = "impl_repo_test_registration_mbx_env_tests.rs"]
mod env_tests;

#[test]
fn nested_workspace_uses_cargo_metadata_target_directory() -> Outcome<()> {
    let root = repo_root().canonicalize()?;
    let document = serde_json::json!({ "target_directory": "/shared/cargo-target" });
    assert_eq!(
        metadata_target_directory(&document, &root)?,
        PathBuf::from("/shared/cargo-target")
    );
    Ok(())
}
