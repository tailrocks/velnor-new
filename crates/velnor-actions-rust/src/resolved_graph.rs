//! Resolved package graph from full `cargo metadata --locked --offline`.
//!
//! Affected-work selection needs every dependency edge Cargo resolved,
//! not only declared path edges between members: a `[patch]`ed or
//! vendored local crate is often reached only through a registry crate
//! (`app -> lapin -> amq-protocol -> amq-protocol-types`). The full
//! resolve lists those edges, plus every local path package (members and
//! non-members alike) with its manifest. Parse failures are errors; the
//! caller selects every package instead of narrowing on a partial graph.

use std::collections::BTreeSet;
use std::path::Path;

use crate::metadata::{METADATA_FORMAT_VERSION, MetadataError, posix};

/// One local path package from the resolve (`source` is null).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResolvedPathPackage {
    /// Opaque Cargo package id.
    pub id: String,
    /// Repository-relative manifest directory; empty for the root.
    pub dir: String,
}

/// Every resolved dependency edge plus every in-repository path package.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedGraph {
    /// `(dependent, dependency)` package-id pairs, sorted unique, all kinds.
    pub edges: Vec<(String, String)>,
    /// Local path packages inside the repository, sorted by id.
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

/// Parse one full (`--locked --offline`, with deps) metadata document.
///
/// `repo_root` must be the same canonical root the discovery parse used.
/// Path packages outside the repository cannot change in a repository
/// diff and are omitted; their edges stay in the graph.
///
/// # Errors
///
/// Returns [`MetadataError::InvalidJson`] for malformed JSON, a missing
/// `resolve` section (a `--no-deps` document), or an edge naming an
/// unlisted package, and [`MetadataError::UnsupportedVersion`] for any
/// other format version.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn document(resolve: &str) -> String {
        format!(
            r#"{{"version":1,"packages":[
                {{"id":"path+file:///repo/apps/a#a@0.1.0","manifest_path":"/repo/apps/a/Cargo.toml","source":null}},
                {{"id":"registry+https://example#lapin@1.0.0","manifest_path":"/home/.cargo/lapin/Cargo.toml","source":"registry+https://example"}},
                {{"id":"path+file:///repo/vendor/types#types@0.1.0","manifest_path":"/repo/vendor/types/Cargo.toml","source":null}},
                {{"id":"path+file:///elsewhere/x#x@0.1.0","manifest_path":"/elsewhere/x/Cargo.toml","source":null}}
            ],"resolve":{resolve}}}"#
        )
    }

    #[test]
    fn registry_edges_reach_vendored_path_packages() {
        let json = document(
            r#"{"nodes":[
                {"id":"path+file:///repo/apps/a#a@0.1.0","dependencies":["registry+https://example#lapin@1.0.0"]},
                {"id":"registry+https://example#lapin@1.0.0","dependencies":["path+file:///repo/vendor/types#types@0.1.0"]},
                {"id":"path+file:///repo/vendor/types#types@0.1.0","dependencies":[]}
            ]}"#,
        );
        let graph = parse_resolved_graph(&json, Path::new("/repo"), "Cargo.toml").expect("parse");
        assert_eq!(graph.edges.len(), 2);
        let dirs: Vec<&str> = graph.path_packages.iter().map(|p| p.dir.as_str()).collect();
        assert_eq!(
            dirs,
            ["apps/a", "vendor/types"],
            "outside-root path packages omitted"
        );
    }

    #[test]
    fn no_deps_documents_fail_closed() {
        let json = document("null");
        assert!(parse_resolved_graph(&json, Path::new("/repo"), "Cargo.toml").is_err());
    }

    #[test]
    fn unlisted_dependencies_fail_closed() {
        let json = document(
            r#"{"nodes":[{"id":"path+file:///repo/apps/a#a@0.1.0","dependencies":["ghost"]}]}"#,
        );
        assert!(parse_resolved_graph(&json, Path::new("/repo"), "Cargo.toml").is_err());
    }
}
