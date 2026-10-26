// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::metadata::{MetadataError, posix};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DepKind {
    Normal,
    Build,
    Dev,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalEdge {
    pub from: String,
    pub to: String,
    pub kind: DepKind,
    pub optional: bool,
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SkippedPathEdge {
    pub from: String,
    pub path: String,
    pub kind: DepKind,
    pub optional: bool,
    pub target: Option<String>,
}

pub(crate) enum EdgeResolution {
    Ignored,
    Edge(LocalEdge),
    Skipped(SkippedPathEdge),
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct RawMetadata {
    pub(crate) version: u32,
    pub(crate) workspace_root: String,
    #[serde(default)]
    pub(crate) workspace_members: Vec<String>,
    #[serde(default)]
    pub(crate) packages: Vec<RawPackage>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct RawPackage {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) id: String,
    pub(crate) manifest_path: String,
    #[serde(default)]
    pub(crate) targets: Vec<RawTarget>,
    #[serde(default)]
    pub(crate) features: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub(crate) dependencies: Vec<RawDependency>,
    #[serde(default)]
    pub(crate) metadata: Option<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct RawTarget {
    #[serde(default)]
    pub(crate) kind: Vec<String>,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) test: bool,
    #[serde(default)]
    pub(crate) doctest: bool,
    #[serde(rename = "required-features", default)]
    pub(crate) required_features: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct RawDependency {
    #[serde(default)]
    pub(crate) kind: Option<String>,
    #[serde(default)]
    pub(crate) optional: bool,
    #[serde(default)]
    pub(crate) target: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) path: Option<String>,
}

#[must_use]
pub fn local_edge_pairs(edges: &[LocalEdge]) -> Vec<(String, String)> {
    edges
        .iter()
        .map(|edge| (edge.from.clone(), edge.to.clone()))
        .collect()
}

pub(crate) fn manifest_dirs(packages: &[RawPackage]) -> BTreeMap<String, String> {
    let mut dirs = BTreeMap::new();
    for package in packages {
        let dir = parent_dir(&package.manifest_path);
        dirs.entry(dir).or_insert_with(|| package.id.clone());
    }
    dirs
}

fn parent_dir(manifest_path: &str) -> String {
    let normalized = manifest_path.replace('\\', "/");
    match normalized.rsplit_once('/') {
        Some((dir, _)) => dir.to_owned(),
        None => normalized,
    }
}

fn normalize_path(path: &str) -> String {
    let dir = path.replace('\\', "/");
    dir.strip_suffix('/')
        .map_or_else(|| dir.clone(), str::to_owned)
}

pub(crate) fn convert_edge(
    from: &str,
    dependency: &RawDependency,
    dirs: &BTreeMap<String, String>,
    hint: &str,
    repo_root: &Path,
    known: &BTreeSet<String>,
) -> Result<EdgeResolution, MetadataError> {
    let Some(path) = dependency.path.as_deref() else {
        return Ok(EdgeResolution::Ignored);
    };
    if let Some(to) = dirs.get(&normalize_path(path)) {
        return Ok(EdgeResolution::Edge(LocalEdge {
            from: from.to_owned(),
            to: to.clone(),
            kind: dep_kind(dependency.kind.as_deref(), hint)?,
            optional: dependency.optional,
            target: dep_target(dependency.target.as_ref(), hint)?,
        }));
    }
    if is_known_target(path, repo_root, known) {
        return Ok(EdgeResolution::Skipped(SkippedPathEdge {
            from: from.to_owned(),
            path: path.to_owned(),
            kind: dep_kind(dependency.kind.as_deref(), hint)?,
            optional: dependency.optional,
            target: dep_target(dependency.target.as_ref(), hint)?,
        }));
    }
    Err(MetadataError::UnresolvedPathEdge {
        manifest: hint.to_owned(),
        from: from.to_owned(),
        path: path.to_owned(),
    })
}

fn is_known_target(path: &str, repo_root: &Path, known: &BTreeSet<String>) -> bool {
    let normalized = normalize_path(path);
    let Ok(relative) = Path::new(&normalized).strip_prefix(repo_root) else {
        return false;
    };
    if relative.as_os_str().is_empty() {
        return known.contains("Cargo.toml");
    }
    known.contains(&format!("{}/Cargo.toml", posix(relative)))
}

fn dep_kind(raw: Option<&str>, hint: &str) -> Result<DepKind, MetadataError> {
    match raw {
        None => Ok(DepKind::Normal),
        Some("build") => Ok(DepKind::Build),
        Some("dev") => Ok(DepKind::Dev),
        Some(other) => Err(MetadataError::UnknownDepKind {
            manifest: hint.to_owned(),
            kind: other.to_owned(),
        }),
    }
}

fn dep_target(
    raw: Option<&serde_json::Value>,
    hint: &str,
) -> Result<Option<String>, MetadataError> {
    match raw {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(cfg)) => Ok(Some(cfg.clone())),
        Some(_) => Err(MetadataError::invalid(
            hint,
            "dependency target must be string-or-null".to_owned(),
        )),
    }
}
