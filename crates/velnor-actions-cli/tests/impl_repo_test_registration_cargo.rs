//! Resolve nested checks to Cargo's workspace target directory.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

use super::{Outcome, PackagePlan, TestTarget, WorkspacePlan};
use crate::impl_repo_policy::{WORKSPACE_ROOTS, repo_root};

fn metadata_target_directory(document: &Value, root: &Path) -> Outcome<PathBuf> {
    let target_directory = PathBuf::from(
        document["target_directory"]
            .as_str()
            .ok_or("metadata target directory")?,
    );
    existing_or_absolute(root, &target_directory)
}

fn cargo() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

fn cargo_output(manifest: &Path, args: &[&str]) -> Outcome<Output> {
    let current_dir = std::env::current_dir()?;
    let manifest_dir = manifest
        .parent()
        .ok_or("workspace manifest has no parent")?;
    let (command, options) = args.split_first().ok_or("empty cargo command")?;
    let mut process = Command::new(cargo());
    process
        .arg(command)
        .arg("--manifest-path")
        .arg(manifest)
        .args(options)
        .current_dir(manifest_dir);
    if let Some(configured) = std::env::var_os("CARGO_TARGET_DIR") {
        process.env(
            "CARGO_TARGET_DIR",
            absolute_path(&current_dir, Path::new(&configured)),
        );
    }
    let output = process.output()?;
    if !output.status.success() {
        return Err(format!(
            "cargo {} failed for {}: {}{}",
            args.first().copied().unwrap_or("command"),
            manifest.display(),
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout),
        )
        .into());
    }
    Ok(output)
}

pub(super) fn workspace_plan(manifest: &Path) -> Outcome<WorkspacePlan> {
    let metadata = cargo_output(
        manifest,
        &["metadata", "--no-deps", "--format-version", "1", "--locked"],
    )?;
    let document: Value = serde_json::from_slice(&metadata.stdout)?;
    let root = manifest
        .parent()
        .ok_or("workspace manifest has no parent")?
        .canonicalize()?;
    let target_directory = metadata_target_directory(&document, &root)?;
    let packages = document["packages"].as_array().ok_or("metadata packages")?;
    let mut plans = Vec::new();
    for package in packages {
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
        let mut has_build_script = false;
        for target in package["targets"].as_array().ok_or("package targets")? {
            has_build_script |= target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "custom-build"));
            if target["test"].as_bool() != Some(true) {
                continue;
            }
            let source = PathBuf::from(
                target["src_path"]
                    .as_str()
                    .ok_or("test target source path")?,
            )
            .canonicalize()?;
            test_targets.push(TestTarget { source });
        }
        plans.push(PackagePlan {
            root: package_root,
            has_build_script,
            test_targets,
        });
    }
    Ok(WorkspacePlan {
        manifest: manifest.canonicalize()?,
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
