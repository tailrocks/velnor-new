// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use crate::metadata_edges::{EdgeResolution, RawPackage, convert_edge, manifest_dirs};

pub const METADATA_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TargetRecord {
    pub kind: String,
    pub name: String,
    pub test: bool,
    pub doctest: bool,
    pub required_features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageRecord {
    pub id: String,
    pub name: String,
    pub version: String,
    pub manifest: String,
    pub external: bool,
    pub in_workspace: bool,
    pub targets: Vec<TargetRecord>,
    pub features: Vec<String>,
    pub has_build_script: bool,
    pub v1_task_owner: VelnorV1TaskOwner,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VelnorV1TaskOwner {
    #[default]
    Project,
    RepositoryMaintenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRecord {
    pub workspace_root: String,
    pub members: Vec<String>,
    pub packages: Vec<PackageRecord>,
    pub edges: Vec<crate::metadata_edges::LocalEdge>,
    pub skipped_edges: Vec<crate::metadata_edges::SkippedPathEdge>,
    pub has_external_dependencies: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataError {
    InvalidJson {
        manifest: String,
        detail: String,
    },
    UnsupportedVersion {
        manifest: String,
        found: u32,
    },
    OutsideRoot {
        manifest: String,
        path: String,
    },
    UnresolvedPathEdge {
        manifest: String,
        from: String,
        path: String,
    },
    UnknownDepKind {
        manifest: String,
        kind: String,
    },
    UnknownVelnorV1TaskOwner {
        manifest: String,
        owner: String,
    },
}

impl MetadataError {
    pub(crate) fn invalid(manifest: &str, detail: String) -> Self {
        Self::InvalidJson {
            manifest: manifest.to_owned(),
            detail,
        }
    }
}

impl fmt::Display for MetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson { manifest, detail } => {
                write!(f, "invalid_metadata:{manifest}: {detail}")
            }
            Self::UnsupportedVersion { manifest, found } => {
                write!(f, "unsupported_version:{manifest}: {found}")
            }
            Self::OutsideRoot { manifest, path } => {
                write!(f, "metadata_outside_root:{manifest}: {path}")
            }
            Self::UnresolvedPathEdge {
                manifest,
                from,
                path,
            } => {
                write!(f, "unresolved_path_edge:{manifest}:{from}: {path}")
            }
            Self::UnknownDepKind { manifest, kind } => {
                write!(f, "unknown_dep_kind:{manifest}: {kind}")
            }
            Self::UnknownVelnorV1TaskOwner { manifest, owner } => {
                write!(f, "unknown_v1_task_owner:{manifest}: {owner}")
            }
        }
    }
}

impl std::error::Error for MetadataError {}

pub fn parse_metadata_json(
    json: &str,
    repo_root: &Path,
    manifest_hint: &str,
    known_manifests: &BTreeSet<String>,
) -> Result<WorkspaceRecord, MetadataError> {
    let raw: crate::metadata_edges::RawMetadata = serde_json::from_str(json)
        .map_err(|err| MetadataError::invalid(manifest_hint, err.to_string()))?;
    if raw.version != METADATA_FORMAT_VERSION {
        return Err(MetadataError::UnsupportedVersion {
            manifest: manifest_hint.to_owned(),
            found: raw.version,
        });
    }
    let members: BTreeSet<&str> = raw.workspace_members.iter().map(String::as_str).collect();
    let mut packages = Vec::with_capacity(raw.packages.len());
    for package in &raw.packages {
        packages.push(convert_package(
            package,
            repo_root,
            &members,
            manifest_hint,
        )?);
    }
    packages.sort_by(|left, right| left.manifest.cmp(&right.manifest));
    let dirs = manifest_dirs(&raw.packages);
    let mut edges = Vec::new();
    let mut skipped = Vec::new();
    for package in &raw.packages {
        for dependency in &package.dependencies {
            match convert_edge(
                &package.id,
                dependency,
                &dirs,
                manifest_hint,
                repo_root,
                known_manifests,
            )? {
                EdgeResolution::Ignored => {}
                EdgeResolution::Edge(edge) => edges.push(edge),
                EdgeResolution::Skipped(edge) => skipped.push(edge),
            }
        }
    }
    edges.sort();
    skipped.sort();
    let has_external_dependencies = raw
        .packages
        .iter()
        .any(|package| package.dependencies.iter().any(|dep| dep.path.is_none()));
    let mut member_ids = raw.workspace_members;
    member_ids.sort();
    Ok(WorkspaceRecord {
        workspace_root: relativize_dir(repo_root, &raw.workspace_root, manifest_hint)?,
        members: member_ids,
        packages,
        edges,
        skipped_edges: skipped,
        has_external_dependencies,
    })
}

fn convert_package(
    raw: &RawPackage,
    repo_root: &Path,
    members: &BTreeSet<&str>,
    manifest_hint: &str,
) -> Result<PackageRecord, MetadataError> {
    let (manifest, external) = relativize_file(repo_root, &raw.manifest_path);
    let mut targets = Vec::new();
    for target in &raw.targets {
        for kind in &target.kind {
            let mut required = target.required_features.clone();
            required.sort();
            targets.push(TargetRecord {
                kind: kind.clone(),
                name: target.name.clone(),
                test: target.test,
                doctest: target.doctest,
                required_features: required,
            });
        }
    }
    targets.sort();
    targets.dedup();
    let mut features: Vec<String> = raw.features.keys().cloned().collect();
    features.sort();
    let v1_task_owner = package_v1_task_owner(raw.metadata.as_ref(), manifest_hint)?;
    Ok(PackageRecord {
        id: raw.id.clone(),
        name: raw.name.clone(),
        version: raw.version.clone(),
        manifest,
        external,
        in_workspace: members.contains(raw.id.as_str()),
        has_build_script: targets.iter().any(|target| target.kind == "custom-build"),
        targets,
        features,
        v1_task_owner,
    })
}

fn package_v1_task_owner(
    metadata: Option<&serde_json::Value>,
    manifest: &str,
) -> Result<VelnorV1TaskOwner, MetadataError> {
    let velnor = metadata
        .and_then(serde_json::Value::as_object)
        .and_then(|metadata| metadata.get("velnor"));
    let Some(velnor) = velnor else {
        return Ok(VelnorV1TaskOwner::Project);
    };
    let Some(velnor) = velnor.as_object() else {
        return Err(MetadataError::UnknownVelnorV1TaskOwner {
            manifest: manifest.to_owned(),
            owner: velnor.to_string(),
        });
    };
    let Some(owner) = velnor.get("v1-task-owner") else {
        return Ok(VelnorV1TaskOwner::Project);
    };
    match owner.as_str() {
        Some("repository-maintenance") => Ok(VelnorV1TaskOwner::RepositoryMaintenance),
        Some(owner) => Err(MetadataError::UnknownVelnorV1TaskOwner {
            manifest: manifest.to_owned(),
            owner: owner.to_owned(),
        }),
        _ => Err(MetadataError::UnknownVelnorV1TaskOwner {
            manifest: manifest.to_owned(),
            owner: owner.to_string(),
        }),
    }
}

fn relativize_file(repo_root: &Path, absolute: &str) -> (String, bool) {
    match Path::new(absolute).strip_prefix(repo_root) {
        Ok(relative) => (posix(relative), false),
        Err(_) => (absolute.replace('\\', "/"), true),
    }
}

fn relativize_dir(repo_root: &Path, absolute: &str, hint: &str) -> Result<String, MetadataError> {
    match Path::new(absolute).strip_prefix(repo_root) {
        Ok(relative) if relative.as_os_str().is_empty() => Ok(String::new()),
        Ok(relative) => Ok(posix(relative)),
        Err(_) => Err(MetadataError::OutsideRoot {
            manifest: hint.to_owned(),
            path: absolute.to_owned(),
        }),
    }
}

pub(crate) fn posix(relative: &Path) -> String {
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
