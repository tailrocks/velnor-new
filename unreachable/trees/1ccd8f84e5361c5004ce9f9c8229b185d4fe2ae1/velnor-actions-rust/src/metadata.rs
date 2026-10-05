//! `cargo metadata` JSON parsing and the conservative local graph.
//!
//! Parse failures are errors; this crate never falls back to manifest parsing.
//! Path targets naming known manifests outside the parsed workspace are
//! skipped as cross-workspace and recorded on the record; unknown targets
//! still fail.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use crate::metadata_edges::{EdgeResolution, RawPackage, convert_edge, manifest_dirs};

/// Expected `cargo metadata --format-version` value.
pub const METADATA_FORMAT_VERSION: u32 = 1;

/// One Cargo target kind entry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TargetRecord {
    /// Target kind (`lib`, `bin`, `test`, `example`, `bench`, `custom-build`).
    pub kind: String,
    /// Target name.
    pub name: String,
    /// Whether `cargo test` exercises this target.
    pub test: bool,
    /// Whether doctests apply to this target.
    pub doctest: bool,
    /// Required features for this target.
    pub required_features: Vec<String>,
}

/// Retained per-package inventory from `cargo metadata`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageRecord {
    /// Opaque Cargo package id.
    pub id: String,
    /// Package name (display only; identity is `id` plus `manifest`).
    pub name: String,
    /// Package version.
    pub version: String,
    /// Repository-relative manifest path, or absolute when outside the root.
    pub manifest: String,
    /// Whether the manifest lives outside the repository root.
    pub external: bool,
    /// Whether the package is a workspace member.
    pub in_workspace: bool,
    /// Targets sorted by `(kind, name)`.
    pub targets: Vec<TargetRecord>,
    /// Sorted declared feature names.
    pub features: Vec<String>,
    /// Whether a `custom-build` target exists.
    pub has_build_script: bool,
}

/// One parsed `cargo metadata` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRecord {
    /// Repository-relative workspace root; empty for the repository root.
    pub workspace_root: String,
    /// Sorted workspace-member package ids.
    pub members: Vec<String>,
    /// Packages sorted by manifest path.
    pub packages: Vec<PackageRecord>,
    /// Local path edges in sorted order.
    pub edges: Vec<crate::metadata_edges::LocalEdge>,
    /// Cross-workspace path edges skipped in sorted order.
    pub skipped_edges: Vec<crate::metadata_edges::SkippedPathEdge>,
}

/// `cargo metadata` parse failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataError {
    /// Input is not valid metadata JSON.
    InvalidJson {
        /// Candidate manifest being parsed.
        manifest: String,
        /// Diagnostic detail.
        detail: String,
    },
    /// Unsupported metadata format version.
    UnsupportedVersion {
        /// Candidate manifest being parsed.
        manifest: String,
        /// Version found in the document.
        found: u32,
    },
    /// A reported workspace path escapes the repository root.
    OutsideRoot {
        /// Candidate manifest being parsed.
        manifest: String,
        /// Offending path.
        path: String,
    },
    /// A declared path edge matches no reported package or known manifest.
    UnresolvedPathEdge {
        /// Candidate manifest being parsed.
        manifest: String,
        /// Dependent package id.
        from: String,
        /// Declared path value.
        path: String,
    },
    /// Unknown dependency kind value.
    UnknownDepKind {
        /// Candidate manifest being parsed.
        manifest: String,
        /// Kind value found.
        kind: String,
    },
}

impl MetadataError {
    /// Build an invalid-input error.
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
        }
    }
}

impl std::error::Error for MetadataError {}

/// Parse one `cargo metadata` document (`repo_root` must be canonical).
///
/// `known_manifests` holds repository-relative POSIX manifest paths
/// discovered in the repository. A declared path target absent from the
/// document but present in `known_manifests` is a real package outside
/// this workspace (nested, parent, or sibling member): skipped and
/// recorded on [`WorkspaceRecord::skipped_edges`], never failed. Any
/// other absent target fails; member inventory is never filtered.
///
/// # Errors
///
/// Returns [`MetadataError`] on any malformed input; no fallback exists.
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
        packages.push(convert_package(package, repo_root, &members));
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
    let mut member_ids = raw.workspace_members;
    member_ids.sort();
    Ok(WorkspaceRecord {
        workspace_root: relativize_dir(repo_root, &raw.workspace_root, manifest_hint)?,
        members: member_ids,
        packages,
        edges,
        skipped_edges: skipped,
    })
}

/// Convert one raw package to retained inventory.
fn convert_package(raw: &RawPackage, repo_root: &Path, members: &BTreeSet<&str>) -> PackageRecord {
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
    PackageRecord {
        id: raw.id.clone(),
        name: raw.name.clone(),
        version: raw.version.clone(),
        manifest,
        external,
        in_workspace: members.contains(raw.id.as_str()),
        has_build_script: targets.iter().any(|target| target.kind == "custom-build"),
        targets,
        features,
    }
}

/// Relativize a reported file path; out-of-root paths stay absolute.
fn relativize_file(repo_root: &Path, absolute: &str) -> (String, bool) {
    match Path::new(absolute).strip_prefix(repo_root) {
        Ok(relative) => (posix(relative), false),
        Err(_) => (absolute.replace('\\', "/"), true),
    }
}

/// Relativize a reported directory; out-of-root workspace paths fail.
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

/// Render a relative path with POSIX separators.
pub(crate) fn posix(relative: &Path) -> String {
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
