//! Cargo/rustc source closure for every registered workspace test target.

#[path = "impl_repo_test_registration_cargo.rs"]
mod cargo_config;
#[path = "impl_repo_test_registration_dep_info.rs"]
mod dep_info;
#[path = "impl_repo_test_registration_fixtures.rs"]
mod fixtures;
#[path = "impl_repo_test_registration_graph.rs"]
mod graph;
#[path = "impl_repo_test_registration_modules.rs"]
mod modules;

use std::collections::{BTreeSet, HashSet};
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::impl_repo_policy::{WORKSPACE_ROOTS, repo_root};

type Outcome<T> = Result<T, Box<dyn Error>>;

struct PackagePlan {
    id: String,
    root: PathBuf,
    test_targets: Vec<PathBuf>,
}

pub(super) struct WorkspacePlan {
    manifest: PathBuf,
    root: PathBuf,
    packages: Vec<PackagePlan>,
}

fn cargo() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

fn cargo_output(manifest: &Path, args: &[&str]) -> Outcome<std::process::Output> {
    let current_dir = manifest
        .parent()
        .ok_or("workspace manifest has no parent")?;
    let (command, options) = args.split_first().ok_or("empty cargo command")?;
    let output = Command::new(cargo())
        .arg(command)
        .arg("--manifest-path")
        .arg(manifest)
        .args(options)
        .env("CARGO_TARGET_DIR", cargo_config::cargo_target_dir()?)
        .current_dir(current_dir)
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
    let metadata = cargo_output(
        manifest,
        &["metadata", "--no-deps", "--format-version", "1", "--locked"],
    )?;
    let document: Value = serde_json::from_slice(&metadata.stdout)?;
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
            if target["test"].as_bool() == Some(true) {
                test_targets.push(
                    PathBuf::from(
                        target["src_path"]
                            .as_str()
                            .ok_or("test target source path")?,
                    )
                    .canonicalize()?,
                );
            }
        }
        plans.push(PackagePlan {
            id,
            root: package_root,
            test_targets,
        });
    }
    let root = manifest
        .parent()
        .ok_or("workspace manifest has no parent")?
        .canonicalize()?;
    Ok(WorkspacePlan {
        manifest: manifest.canonicalize()?,
        root,
        packages: plans,
    })
}

fn workspace_plans(root: &Path) -> Outcome<Vec<WorkspacePlan>> {
    WORKSPACE_ROOTS
        .iter()
        .map(|workspace| workspace_plan(&root.join(workspace).join("Cargo.toml")))
        .collect()
}

fn registered_sources(workspaces: &[WorkspacePlan]) -> Outcome<HashSet<PathBuf>> {
    let mut registered = HashSet::new();
    for workspace in workspaces {
        registered.extend(compiled_test_sources(workspace)?);
    }
    Ok(registered)
}

fn compiled_test_sources(workspace: &WorkspacePlan) -> Outcome<HashSet<PathBuf>> {
    let output = cargo_output(
        &workspace.manifest,
        &[
            "test",
            "--no-run",
            "--locked",
            "--workspace",
            "--message-format=json",
        ],
    )?;
    let package_ids: HashSet<&str> = workspace
        .packages
        .iter()
        .map(|package| package.id.as_str())
        .collect();
    let mut sources = HashSet::new();
    for line in output.stdout.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let message: Value = serde_json::from_slice(line)?;
        if let Some(artifact) = test_artifact(&message, &package_ids, workspace)? {
            sources.extend(modules::source_closure(
                &artifact.source,
                &artifact.dependencies,
            )?);
        }
    }
    for package in &workspace.packages {
        for target in &package.test_targets {
            if !sources.contains(target) {
                return Err(format!(
                    "Cargo metadata test target source is absent from rustc dependency files: {}",
                    target.display()
                )
                .into());
            }
        }
    }
    Ok(sources)
}

struct TestArtifact {
    source: PathBuf,
    dependencies: HashSet<PathBuf>,
}

fn test_artifact(
    message: &Value,
    package_ids: &HashSet<&str>,
    workspace: &WorkspacePlan,
) -> Outcome<Option<TestArtifact>> {
    if message["reason"] != "compiler-artifact"
        || message["target"]["test"].as_bool() != Some(true)
        || message["profile"]["test"].as_bool() != Some(true)
    {
        return Ok(None);
    }
    let package_id = message["package_id"]
        .as_str()
        .ok_or("artifact package id")?;
    if !package_ids.contains(package_id) {
        return Ok(None);
    }
    let Some(executable) = message["executable"].as_str() else {
        return Ok(None);
    };
    let dep_info = PathBuf::from(executable).with_extension("d");
    if !dep_info.is_file() {
        if Path::new(executable)
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|directory| directory == "deps")
        {
            return Err(format!(
                "Cargo test executable is missing dep-info: {}",
                dep_info.display()
            )
            .into());
        }
        return Ok(None);
    }
    let dependencies: HashSet<PathBuf> = dep_info::sources(&dep_info, &workspace.root)?
        .into_iter()
        .collect();
    let source = PathBuf::from(
        message["target"]["src_path"]
            .as_str()
            .ok_or("test artifact source path")?,
    )
    .canonicalize()?;
    if !dependencies.contains(&source) {
        return Err(format!(
            "test-profile dep-info omits Cargo test source {}",
            source.display()
        )
        .into());
    }
    Ok(Some(TestArtifact {
        source,
        dependencies,
    }))
}

fn rust_files(package_root: &Path) -> Outcome<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![package_root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                if !excluded_directory(&path) {
                    pending.push(path);
                }
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path.canonicalize()?);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn excluded_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "target" | ".git"))
}

pub(super) fn registration_audit(
    workspaces: &[WorkspacePlan],
) -> Outcome<(HashSet<PathBuf>, Vec<PathBuf>)> {
    let registered = registered_sources(workspaces)?;
    let mut pending = BTreeSet::new();
    for package in workspaces.iter().flat_map(|workspace| &workspace.packages) {
        pending.extend(rust_files(&package.root)?);
    }
    let mut visited = HashSet::new();
    let mut candidates = HashSet::new();
    let mut source_records = Vec::new();
    while let Some(path) = pending.pop_first() {
        let path = path.canonicalize()?;
        if !visited.insert(path.clone()) {
            continue;
        }
        let findings = graph::source_findings(&path)
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        if findings.test_bearing {
            candidates.insert(path.clone());
        }
        let macro_includes = graph::active_macro_includes(&findings)?;
        for include in &findings.includes {
            if include.path.is_file() {
                pending.insert(include.path.clone());
            }
        }
        for (include, _) in macro_includes {
            if include.is_file() {
                pending.insert(include);
            }
        }
        source_records.push((path, findings));
    }
    add_macro_test_sources(&source_records, &mut candidates);
    let mut orphans: Vec<PathBuf> = candidates.difference(&registered).cloned().collect();
    orphans.sort();
    Ok((registered, orphans))
}

fn add_macro_test_sources(
    records: &[(PathBuf, graph::SourceFindings)],
    candidates: &mut HashSet<PathBuf>,
) {
    let invoked = records
        .iter()
        .flat_map(|(_, findings)| &findings.macro_calls)
        .filter(|call| call.condition != graph::Possibility::Never)
        .map(|call| (call.name.clone(), call.scope.clone()))
        .collect::<HashSet<_>>();
    for (source, findings) in records {
        for definition in &findings.macro_definitions {
            if !definition.test_bearing || definition.condition == graph::Possibility::Never {
                continue;
            }
            if invoked.contains(&(definition.name.clone(), definition.scope.clone())) {
                candidates.insert(source.clone());
            }
        }
    }
}

pub(super) fn relative_paths(root: &Path, paths: &[PathBuf]) -> Outcome<Vec<String>> {
    paths
        .iter()
        .map(|path| {
            Ok(path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/"))
        })
        .collect()
}

#[test]
fn cargo_targets_register_every_test_bearing_source() -> Outcome<()> {
    let root = repo_root().canonicalize()?;
    let workspaces = workspace_plans(&root)?;
    let (registered, orphans) = registration_audit(&workspaces)?;
    for expected in [
        "crates/velnor-actions-orchestrator/tests/impl_generator_seed.rs",
        "crates/velnor-actions-tofu/tests/impl_tofu_t27_select.rs",
        "crates/velnor-actions-tofu/tests/impl_tofu_file_cache.rs",
    ] {
        assert!(
            registered.contains(&root.join(expected).canonicalize()?),
            "real integration test source is absent from Cargo/rustc test closure: {expected}"
        );
    }
    assert!(
        orphans.is_empty(),
        "test-bearing Rust sources are not compiled by Cargo test targets: {:?}",
        relative_paths(&root, &orphans)?
    );
    Ok(())
}
