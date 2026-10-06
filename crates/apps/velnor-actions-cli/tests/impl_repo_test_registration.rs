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
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::impl_repo_policy::repo_root;

type Outcome<T> = Result<T, Box<dyn Error>>;

struct PackagePlan {
    id: String,
    root: PathBuf,
    test_targets: Vec<TestTarget>,
}

struct TestTarget {
    source: PathBuf,
    required_features: Vec<String>,
}

pub(super) struct WorkspacePlan {
    manifest: PathBuf,
    root: PathBuf,
    target_directory: PathBuf,
    vcs_metadata: Option<PathBuf>,
    packages: Vec<PackagePlan>,
}

fn registered_sources(workspaces: &[WorkspacePlan]) -> Outcome<HashSet<PathBuf>> {
    let mut registered = HashSet::new();
    for workspace in workspaces {
        registered.extend(compiled_test_sources(workspace)?);
    }
    Ok(registered)
}

fn compiled_test_sources(workspace: &WorkspacePlan) -> Outcome<HashSet<PathBuf>> {
    let output = cargo_config::cargo_output_at_target(
        &workspace.manifest,
        &[
            "test",
            "--no-run",
            "--locked",
            "--workspace",
            "--message-format=json",
        ],
        &workspace.target_directory,
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
            if sources.contains(&target.source) {
                continue;
            }
            if target.required_features.is_empty() {
                return Err(format!(
                    "Cargo metadata test target source is absent from rustc dependency files: {}",
                    target.source.display()
                )
                .into());
            }
            sources.extend(modules::declared_target_source_closure(&target.source)?);
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

fn rust_files(
    package_root: &Path,
    target_directory: &Path,
    vcs_metadata: Option<&Path>,
) -> Outcome<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![package_root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                if !excluded_directory(&path, target_directory, vcs_metadata)? {
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

fn excluded_directory(
    path: &Path,
    target_directory: &Path,
    vcs_metadata: Option<&Path>,
) -> Outcome<bool> {
    let canonical = path.canonicalize()?;
    Ok(canonical == target_directory || Some(canonical.as_path()) == vcs_metadata)
}

pub(super) fn registration_audit(
    workspaces: &[WorkspacePlan],
) -> Outcome<(HashSet<PathBuf>, Vec<PathBuf>)> {
    let registered = registered_sources(workspaces)?;
    let mut pending = BTreeSet::new();
    for workspace in workspaces {
        for package in &workspace.packages {
            pending.extend(rust_files(
                &package.root,
                &workspace.target_directory,
                workspace.vcs_metadata.as_deref(),
            )?);
        }
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
    let workspaces = cargo_config::workspace_plans(&root)?;
    let (registered, orphans) = registration_audit(&workspaces)?;
    for expected in [
        "crates/services/velnor-actions-orchestrator/tests/impl_generator_seed.rs",
        "crates/adapters/velnor-actions-tofu/tests/impl_tofu_t27_select.rs",
        "crates/adapters/velnor-actions-tofu/tests/impl_tofu_file_cache.rs",
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
