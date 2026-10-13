//! Local path-edge resolution for one `cargo metadata` document.
//!
//! Edges resolve only between packages reported in the same document:
//! `cargo metadata --no-deps` lists workspace members and nothing else.
//! A declared path target naming a known manifest outside this workspace
//! (a nested, parent, or sibling workspace member) is definitionally not
//! part of this workspace's graph: it is skipped and recorded, never
//! failed. Targets naming nothing known still fail as
//! [`MetadataError::UnresolvedPathEdge`](crate::metadata::MetadataError).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::metadata::{MetadataError, posix};

/// Dependency edge kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DepKind {
    /// Normal dependency.
    Normal,
    /// Build dependency.
    Build,
    /// Development dependency.
    Dev,
}

/// One declared local path edge between first-party packages.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalEdge {
    /// Cargo package id of the dependent.
    pub from: String,
    /// Cargo package id of the path dependency.
    pub to: String,
    /// Dependency kind.
    pub kind: DepKind,
    /// Whether the edge is optional.
    pub optional: bool,
    /// Target filter when target-specific.
    pub target: Option<String>,
}

/// One declared path edge skipped as cross-workspace.
///
/// The target names a known manifest outside this workspace, so the edge
/// cannot join this workspace's local graph. Recorded so the skip is
/// never silent; unknown targets fail parsing instead of landing here.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SkippedPathEdge {
    /// Cargo package id of the dependent.
    pub from: String,
    /// Declared path value of the dependency.
    pub path: String,
    /// Dependency kind.
    pub kind: DepKind,
    /// Whether the edge is optional.
    pub optional: bool,
    /// Target filter when target-specific.
    pub target: Option<String>,
}

/// Resolution of one declared dependency.
pub(crate) enum EdgeResolution {
    /// Non-path dependency (no local edge).
    Ignored,
    /// Intra-workspace edge between reported packages.
    Edge(LocalEdge),
    /// Cross-workspace edge to a known outside manifest.
    Skipped(SkippedPathEdge),
}

/// Raw `cargo metadata` document (unlisted fields ignored).
#[derive(Debug, serde::Deserialize)]
pub(crate) struct RawMetadata {
    pub(crate) version: u32,
    pub(crate) workspace_root: String,
    #[serde(default)]
    pub(crate) workspace_members: Vec<String>,
    #[serde(default)]
    pub(crate) packages: Vec<RawPackage>,
}

/// Raw package entry.
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
    #[serde(default)]
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

/// Project local edges to `(from, to)` key pairs for the neutral closure.
///
/// Single owner of the [`LocalEdge`] projection into
/// [`reverse_closure`](velnor_actions_contract_planning::reverse_closure); kind,
/// optionality, and target filters never affect selection.
#[must_use]
pub fn local_edge_pairs(edges: &[LocalEdge]) -> Vec<(String, String)> {
    edges
        .iter()
        .map(|edge| (edge.from.clone(), edge.to.clone()))
        .collect()
}

/// Map each package manifest directory to its package id.
pub(crate) fn manifest_dirs(packages: &[RawPackage]) -> BTreeMap<String, String> {
    let mut dirs = BTreeMap::new();
    for package in packages {
        let dir = parent_dir(&package.manifest_path);
        dirs.entry(dir).or_insert_with(|| package.id.clone());
    }
    dirs
}

/// Parent directory of one manifest path, slash-normalized.
fn parent_dir(manifest_path: &str) -> String {
    let normalized = manifest_path.replace('\\', "/");
    match normalized.rsplit_once('/') {
        Some((dir, _)) => dir.to_owned(),
        None => normalized,
    }
}

/// Declared path value normalized for directory comparison.
fn normalize_path(path: &str) -> String {
    let dir = path.replace('\\', "/");
    dir.strip_suffix('/')
        .map_or_else(|| dir.clone(), str::to_owned)
}

/// Resolve one dependency declaration against this document's packages.
///
/// `known` holds repository-relative POSIX manifest paths discovered in
/// the repository. A path target absent from `dirs` but present in
/// `known` is a real package outside this workspace: skipped, recorded,
/// never failed. Any other absent target fails as before.
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

/// Whether `path` names a known manifest directory inside the repository.
///
/// `path` is the declared dependency path (absolute, per Cargo); `known`
/// holds repository-relative POSIX manifest paths. Outside-root paths
/// are never known: discovery cannot verify what it cannot see.
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

/// Map a raw dependency kind (`null` means normal).
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

/// Map a raw target filter (`null` or string).
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
