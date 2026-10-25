use std::collections::BTreeSet;
use std::path::Path;

use crate::metadata::{METADATA_FORMAT_VERSION, MetadataError, posix};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResolvedPathPackage {
    pub id: String,
    pub dir: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedGraph {
    pub edges: Vec<(String, String)>,
    pub path_packages: Vec<ResolvedPathPackage>,
}

#[derive(Debug, serde::Deserialize)]
struct RawResolvedMetadata {
    version: u32,
    #[serde(default)]
    packages: Vec<RawResolvedPackage>,
    resolve: Option<RawResolve>,
}

#[derive(Debug, serde::Deserialize)]
struct RawResolvedPackage {
    id: String,
    manifest_path: String,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RawResolve {
    #[serde(default)]
    nodes: Vec<RawNode>,
}

#[derive(Debug, serde::Deserialize)]
struct RawNode {
    id: String,
    #[serde(default)]
    dependencies: Vec<String>,
}

pub fn parse_resolved_graph(
    json: &str,
    repo_root: &Path,
    manifest_hint: &str,
) -> Result<ResolvedGraph, MetadataError> {
    let raw: RawResolvedMetadata = serde_json::from_str(json)
        .map_err(|err| MetadataError::invalid(manifest_hint, err.to_string()))?;
    if raw.version != METADATA_FORMAT_VERSION {
        return Err(MetadataError::UnsupportedVersion {
            manifest: manifest_hint.to_owned(),
            found: raw.version,
        });
    }
    let Some(resolve) = raw.resolve else {
        return Err(MetadataError::invalid(
            manifest_hint,
            "missing_resolve".to_owned(),
        ));
    };
    let known: BTreeSet<&str> = raw.packages.iter().map(|pkg| pkg.id.as_str()).collect();
    let mut edges = BTreeSet::new();
    for node in &resolve.nodes {
        if !known.contains(node.id.as_str()) {
            return Err(MetadataError::invalid(
                manifest_hint,
                format!("unlisted_resolve_node:{}", node.id),
            ));
        }
        for dependency in &node.dependencies {
            if !known.contains(dependency.as_str()) {
                return Err(MetadataError::invalid(
                    manifest_hint,
                    format!("unlisted_resolve_dependency:{dependency}"),
                ));
            }
            edges.insert((node.id.clone(), dependency.clone()));
        }
    }
    let mut path_packages = Vec::new();
    for package in raw.packages.iter().filter(|pkg| pkg.source.is_none()) {
        let manifest = Path::new(&package.manifest_path);
        let Ok(relative) = manifest.strip_prefix(repo_root) else {
            continue;
        };
        let dir = relative.parent().map(posix).unwrap_or_default();
        path_packages.push(ResolvedPathPackage {
            id: package.id.clone(),
            dir,
        });
    }
    path_packages.sort();
    Ok(ResolvedGraph {
        edges: edges.into_iter().collect(),
        path_packages,
    })
}
