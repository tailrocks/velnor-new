// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use velnor_model::{
    DependencyKind, PackageDependency, PackageName, RepoPath, RepoView, RustMetadata, RustPackage,
    Target, TargetKind, TargetName,
};

use crate::detect::metadata_roots;
use crate::sha256::sha256_hex;

pub const METADATA_PATH: RepoPath = RepoPath::from_static(".velnor/rust-metadata.json");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataFileError {
    Missing,
    Unreadable(String),
    Changed(RepoPath),
    Unlisted(RepoPath),
    Differs(RepoPath),
    Layout,
}

impl fmt::Display for MetadataFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(f, "{METADATA_PATH} is missing"),
            Self::Unreadable(detail) => write!(f, "{METADATA_PATH} is unreadable: {detail}"),
            Self::Changed(path) => write!(f, "{path} changed after {METADATA_PATH} was written"),
            Self::Unlisted(path) => write!(f, "{path} is not in {METADATA_PATH}"),
            Self::Differs(path) => write!(
                f,
                "{METADATA_PATH} differs from what cargo reports for {path}"
            ),
            Self::Layout => write!(
                f,
                "a file Cargo discovers as a target was added or removed after {METADATA_PATH} was written"
            ),
        }?;
        f.write_str("; run velnor-gen generate")
    }
}

impl std::error::Error for MetadataFileError {}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    roots: BTreeMap<String, Root>,
    manifests: BTreeMap<String, String>,
    layout: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Root {
    members: Vec<Member>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Member {
    name: String,
    version: String,
    manifest: String,
    dependencies: Vec<Edge>,
    targets: Vec<Goal>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Edge {
    name: String,
    kind: String,
    manifest: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    features: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rename: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Goal {
    name: String,
    kinds: Vec<String>,
    tests: bool,
    doctest: bool,
    source: Option<String>,
    requires_features: bool,
}

const KINDS: [(&str, TargetKind); 7] = [
    ("lib", TargetKind::Lib),
    ("proc-macro", TargetKind::ProcMacro),
    ("bin", TargetKind::Bin),
    ("test", TargetKind::Test),
    ("bench", TargetKind::Bench),
    ("example", TargetKind::Example),
    ("build-script", TargetKind::BuildScript),
];

const DEPENDENCIES: [(&str, DependencyKind); 3] = [
    ("normal", DependencyKind::Normal),
    ("dev", DependencyKind::Dev),
    ("build", DependencyKind::Build),
];

fn kind_name(kind: TargetKind) -> &'static str {
    KINDS
        .iter()
        .find(|(_, known)| *known == kind)
        .map_or("lib", |(name, _)| name)
}

fn dependency_name(kind: DependencyKind) -> &'static str {
    DEPENDENCIES
        .iter()
        .find(|(_, known)| *known == kind)
        .map_or("normal", |(name, _)| name)
}

pub fn metadata_file(view: &RepoView) -> Result<String, MetadataFileError> {
    let mut manifests = BTreeMap::new();
    let mut roots = BTreeMap::new();
    for (workspace, metadata) in &view.rust_metadata {
        for path in std::iter::once(workspace).chain(metadata.members.iter().map(|m| &m.manifest)) {
            let bytes = view
                .files
                .get(path)
                .ok_or_else(|| MetadataFileError::Changed(path.clone()))?;
            manifests.insert(path.to_string(), sha256_hex(bytes));
        }
        roots.insert(
            workspace.to_string(),
            Root {
                members: metadata
                    .members
                    .iter()
                    .map(member)
                    .collect::<Result<_, _>>()?,
            },
        );
    }
    let mut text = serde_json::to_string_pretty(&File {
        roots,
        manifests,
        layout: layout(&view.tracked_paths, view.rust_metadata.values()),
    })
    .map_err(|error| MetadataFileError::Unreadable(error.to_string()))?;
    text.push('\n');
    Ok(text)
}

fn member(package: &RustPackage) -> Result<Member, MetadataFileError> {
    let version = package
        .version
        .clone()
        .ok_or_else(|| MetadataFileError::Unreadable(format!("{} has no version", package.name)))?;
    Ok(Member {
        name: package.name.to_string(),
        version,
        manifest: package.manifest.to_string(),
        dependencies: package
            .dependencies
            .iter()
            .map(|dependency| Edge {
                name: dependency.name.to_string(),
                kind: dependency_name(dependency.kind).to_owned(),
                manifest: dependency.manifest.as_ref().map(ToString::to_string),
                features: dependency.features.clone(),
                platform: dependency.platform.clone(),
                rename: dependency.rename.clone(),
            })
            .collect(),
        targets: package
            .targets
            .iter()
            .map(|target| Goal {
                name: target.name.to_string(),
                kinds: target
                    .kinds
                    .iter()
                    .map(|kind| kind_name(*kind).to_owned())
                    .collect(),
                tests: target.tests,
                doctest: target.doctest,
                source: target.source.as_ref().map(ToString::to_string),
                requires_features: target.requires_features,
            })
            .collect(),
    })
}

pub fn recorded_metadata(
    view: &RepoView,
    recorded: Option<&[u8]>,
) -> Result<BTreeMap<RepoPath, RustMetadata>, MetadataFileError> {
    let bytes = recorded.ok_or(MetadataFileError::Missing)?;
    let file: File = serde_json::from_slice(bytes)
        .map_err(|error| MetadataFileError::Unreadable(error.to_string()))?;
    for (path, digest) in &file.manifests {
        let path = path_of(path)?;
        let current = view.files.get(&path).map(|bytes| sha256_hex(bytes));
        if current.as_deref() != Some(digest.as_str()) {
            return Err(MetadataFileError::Changed(path));
        }
    }
    let mut known = BTreeMap::new();
    for (workspace, root) in file.roots {
        let workspace = path_of(&workspace)?;
        let members = root
            .members
            .into_iter()
            .map(package)
            .collect::<Result<Vec<_>, _>>()?;
        for manifest in std::iter::once(&workspace).chain(members.iter().map(|m| &m.manifest)) {
            if !file.manifests.contains_key(manifest.as_str()) {
                return Err(MetadataFileError::Unlisted(manifest.clone()));
            }
        }
        known.insert(workspace, RustMetadata { members });
    }
    if file.layout != layout(&view.tracked_paths, known.values()) {
        return Err(MetadataFileError::Layout);
    }
    let candidates =
        metadata_roots(view).map_err(|error| MetadataFileError::Unreadable(error.to_string()))?;
    for manifest in candidates {
        let listed = known.iter().any(|(workspace, metadata)| {
            *workspace == manifest || metadata.members.iter().any(|m| m.manifest == manifest)
        });
        if !listed {
            return Err(MetadataFileError::Unlisted(manifest));
        }
    }
    Ok(known)
}

const DISCOVERED_FILES: [&str; 3] = ["build.rs", "src/lib.rs", "src/main.rs"];
const DISCOVERED_FOLDERS: [&str; 4] = ["src/bin", "tests", "examples", "benches"];

fn discovered(relative: &str) -> bool {
    DISCOVERED_FILES.contains(&relative)
        || DISCOVERED_FOLDERS.iter().any(|folder| {
            relative
                .strip_prefix(folder)
                .and_then(|rest| rest.strip_prefix('/'))
                .is_some_and(|rest| {
                    rest.strip_suffix(".rs")
                        .is_some_and(|name| !name.contains('/'))
                        || rest
                            .strip_suffix("/main.rs")
                            .is_some_and(|name| !name.contains('/'))
                })
        })
}

fn layout<'a>(
    tracked: &BTreeSet<RepoPath>,
    metadata: impl Iterator<Item = &'a RustMetadata>,
) -> String {
    let directories: BTreeSet<String> = metadata
        .flat_map(|metadata| &metadata.members)
        .map(|member| {
            member
                .manifest
                .parent()
                .map_or_else(String::new, |d| format!("{d}/"))
        })
        .collect();
    let found: Vec<&str> = tracked
        .iter()
        .map(RepoPath::as_str)
        .filter(|path| {
            directories.iter().any(|directory| {
                path.strip_prefix(directory.as_str())
                    .is_some_and(discovered)
            })
        })
        .collect();
    sha256_hex(found.join("\n").as_bytes())
}

fn path_of(text: &str) -> Result<RepoPath, MetadataFileError> {
    RepoPath::new(text).map_err(|error| MetadataFileError::Unreadable(error.to_string()))
}

fn package(member: Member) -> Result<RustPackage, MetadataFileError> {
    let unreadable =
        |error: velnor_model::InvalidId| MetadataFileError::Unreadable(error.to_string());
    Ok(RustPackage {
        name: PackageName::new(member.name).map_err(unreadable)?,
        version: Some(member.version),
        manifest: path_of(&member.manifest)?,
        dependencies: member
            .dependencies
            .into_iter()
            .map(|edge| {
                Ok(PackageDependency {
                    name: PackageName::new(edge.name).map_err(unreadable)?,
                    kind: choose(&DEPENDENCIES, &edge.kind)?,
                    manifest: edge.manifest.as_deref().map(path_of).transpose()?,
                    features: edge.features,
                    platform: edge.platform,
                    rename: edge.rename,
                })
            })
            .collect::<Result<_, MetadataFileError>>()?,
        targets: member
            .targets
            .into_iter()
            .map(|goal| {
                Ok(Target {
                    name: TargetName::new(goal.name).map_err(unreadable)?,
                    kinds: goal
                        .kinds
                        .iter()
                        .map(|kind| choose(&KINDS, kind))
                        .collect::<Result<_, _>>()?,
                    tests: goal.tests,
                    doctest: goal.doctest,
                    source: goal.source.as_deref().map(path_of).transpose()?,
                    requires_features: goal.requires_features,
                })
            })
            .collect::<Result<_, MetadataFileError>>()?,
    })
}

fn choose<T: Copy>(table: &[(&str, T)], name: &str) -> Result<T, MetadataFileError> {
    table
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, value)| *value)
        .ok_or_else(|| MetadataFileError::Unreadable(format!("unknown kind {name}")))
}
