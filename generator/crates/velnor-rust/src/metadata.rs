// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::reads::{Located, locate};
use velnor_model::{
    DependencyKind, PackageDependency, PackageName, RepoPath, RustMetadata, RustPackage, Target,
    TargetKind, TargetName,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataError {
    Invalid(String),
    OutsideRepository(PathBuf),
}

impl fmt::Display for MetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(f, "cargo metadata: {message}"),
            Self::OutsideRepository(path) => {
                write!(
                    f,
                    "cargo metadata: {} is outside the repository",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for MetadataError {}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_root: PathBuf,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    manifest_path: PathBuf,
    dependencies: Vec<Dependency>,
    targets: Vec<RawTarget>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
    path: Option<PathBuf>,
}

#[derive(Deserialize)]
struct RawTarget {
    name: String,
    kind: Vec<String>,
    #[serde(default)]
    test: bool,
    src_path: Option<PathBuf>,
    #[serde(rename = "required-features", default)]
    required_features: Vec<String>,
}

pub fn reduce_metadata(
    repository: &Path,
    json: &[u8],
) -> Result<(RepoPath, RustMetadata), MetadataError> {
    let metadata: Metadata = serde_json::from_slice(json).map_err(invalid)?;
    let workspace = repo_path(repository, &metadata.workspace_root.join("Cargo.toml"))?;
    let members = metadata
        .packages
        .into_iter()
        .map(|package| {
            Ok(RustPackage {
                name: PackageName::new(package.name).map_err(invalid)?,
                manifest: repo_path(repository, &package.manifest_path)?,
                dependencies: package
                    .dependencies
                    .into_iter()
                    .map(|dependency| dependency_of(repository, dependency))
                    .collect::<Result<_, _>>()?,
                targets: package
                    .targets
                    .into_iter()
                    .map(|target| target_of(repository, target))
                    .collect::<Result<_, _>>()?,
            })
        })
        .collect::<Result<_, MetadataError>>()?;
    Ok((workspace, RustMetadata { members }))
}

fn dependency_of(
    repository: &Path,
    dependency: Dependency,
) -> Result<PackageDependency, MetadataError> {
    let kind = match dependency.kind.as_deref() {
        None => DependencyKind::Normal,
        Some("dev") => DependencyKind::Dev,
        Some("build") => DependencyKind::Build,
        Some(other) => return Err(invalid(format!("unknown dependency kind {other}"))),
    };
    Ok(PackageDependency {
        name: PackageName::new(dependency.name).map_err(invalid)?,
        kind,
        manifest: dependency
            .path
            .map(|path| repo_path(repository, &path.join("Cargo.toml")))
            .transpose()?,
    })
}

fn target_of(repository: &Path, target: RawTarget) -> Result<Target, MetadataError> {
    let kinds = target
        .kind
        .iter()
        .map(|kind| match kind.as_str() {
            "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" => Ok(TargetKind::Lib),
            "proc-macro" => Ok(TargetKind::ProcMacro),
            "bin" => Ok(TargetKind::Bin),
            "test" => Ok(TargetKind::Test),
            "bench" => Ok(TargetKind::Bench),
            "example" => Ok(TargetKind::Example),
            "custom-build" => Ok(TargetKind::BuildScript),
            other => Err(invalid(format!("unknown target kind {other}"))),
        })
        .collect::<Result<_, _>>()?;
    Ok(Target {
        name: TargetName::new(target.name).map_err(invalid)?,
        kinds,
        tests: target.test,
        requires_features: !target.required_features.is_empty(),
        source: target
            .src_path
            .and_then(|path| match locate(repository, &path, "") {
                Located::Inside(source) => Some(source),
                Located::Repository | Located::Outside => None,
            }),
    })
}

fn repo_path(repository: &Path, path: &Path) -> Result<RepoPath, MetadataError> {
    let inside = path
        .strip_prefix(repository)
        .ok()
        .filter(|inside| {
            inside
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
        })
        .ok_or_else(|| MetadataError::OutsideRepository(path.to_owned()))?;
    let text = inside
        .to_str()
        .ok_or_else(|| invalid(format!("{} is not UTF-8", inside.display())))?;
    RepoPath::new(text).map_err(invalid)
}

fn invalid(error: impl fmt::Display) -> MetadataError {
    MetadataError::Invalid(error.to_string())
}
