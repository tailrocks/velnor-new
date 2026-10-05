//! Keep nested Cargo checks outside the source worktree.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

use super::{Outcome, PackagePlan, TestTarget, WorkspacePlan};
use crate::impl_repo_policy::{WORKSPACE_ROOTS, repo_root};

pub(super) fn cargo_target_dir() -> std::io::Result<PathBuf> {
    Ok(selected_target_dir(
        std::env::var_os("CARGO_TARGET_DIR"),
        std::env::var_os("CARGO_TARGET_TMPDIR"),
        &std::env::current_dir()?,
        &std::env::temp_dir(),
        std::process::id(),
    ))
}

fn selected_target_dir(
    configured: Option<OsString>,
    test_temp: Option<OsString>,
    current_dir: &Path,
    temp_dir: &Path,
    process_id: u32,
) -> PathBuf {
    let target = configured.or(test_temp).map_or_else(
        || temp_dir.join(format!("velnor-test-registration-{process_id}")),
        PathBuf::from,
    );
    if target.is_absolute() {
        target
    } else {
        current_dir.join(target)
    }
}

fn cargo() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

pub(super) fn cargo_output_at_target(
    manifest: &Path,
    args: &[&str],
    target_dir: &Path,
) -> Outcome<Output> {
    let current_dir = std::env::current_dir()?;
    let manifest_dir = manifest
        .parent()
        .ok_or("workspace manifest has no parent")?;
    let (command, options) = args.split_first().ok_or("empty cargo command")?;
    let target_dir = absolute_path(&current_dir, target_dir);
    let output = Command::new(cargo())
        .arg(command)
        .arg("--manifest-path")
        .arg(manifest)
        .args(options)
        .env("CARGO_TARGET_DIR", target_dir)
        .current_dir(manifest_dir)
        .output()?;
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
    let target_dir = cargo_target_dir()?;
    workspace_plan_at_target(manifest, &target_dir)
}

pub(super) fn workspace_plan_at_target(
    manifest: &Path,
    target_dir: &Path,
) -> Outcome<WorkspacePlan> {
    let metadata = cargo_output_at_target(
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

#[test]
fn nested_cargo_target_is_absolute_and_anchored_before_chdir() {
    let checkout = Path::new("/checkout");
    let temp = Path::new("/task-tmp");
    assert_eq!(
        selected_target_dir(
            Some(OsString::from("cache/cargo")),
            None,
            checkout,
            temp,
            17,
        ),
        checkout.join("cache/cargo")
    );
    assert_eq!(
        selected_target_dir(
            None,
            Some(OsString::from("/outer/target/tmp")),
            checkout,
            temp,
            17
        ),
        Path::new("/outer/target/tmp")
    );
    assert_eq!(
        selected_target_dir(None, None, checkout, temp, 17),
        temp.join("velnor-test-registration-17")
    );
}
