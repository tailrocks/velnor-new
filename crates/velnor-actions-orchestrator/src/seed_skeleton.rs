//! `seed-skeleton-v1`: stub workspace member sources in place.
//!
//! Reads the resolve graph the generated step wrote with `cargo metadata
//! --format-version 1 --locked --offline --all-features`, then rewrites the
//! source file of every workspace member target, in the seed's own checkout
//! (the same absolute path as the crate jobs), so member code never
//! compiles: lib-like targets get a doc-only stub, everything else
//! (bins, tests, benches, examples, build scripts) a `fn main() {}` stub.
//! Both compile warning-free under `-D warnings`, Clippy pedantic, and
//! `missing_docs`. Path packages a registry or Git package depends on
//! (`[patch]` targets) and their path-dependency closure keep their real
//! sources, so the registry units built on them stay correct. Manifests and
//! `Cargo.lock` are never touched.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::OrchestratorError;
use crate::internal::internal;

/// Stub of a library-like target.
pub(crate) const LIB_STUB: &str = "//! Velnor dependency seed stub.\n";
/// Stub of an executable-like target (bin, test, bench, example, build script).
pub(crate) const BIN_STUB: &str = "//! Velnor dependency seed stub.\n\nfn main() {}\n";
/// Stub of one file shared by library-like and executable-like targets:
/// a documented public `main` is neither dead code in the library nor
/// undocumented under `missing_docs`.
pub(crate) const SHARED_STUB: &str =
    "//! Velnor dependency seed stub.\n\n/// Seed stub entry point.\npub fn main() {}\n";

const LIB_KINDS: [&str; 6] = ["lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"];

/// The subset of `cargo metadata` the seed reads.
#[derive(Debug, Deserialize)]
pub(crate) struct Metadata {
    pub(crate) packages: Vec<Package>,
    pub(crate) workspace_members: Vec<String>,
    pub(crate) resolve: Option<Resolve>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Package {
    pub(crate) id: String,
    pub(crate) source: Option<String>,
    #[serde(default)]
    pub(crate) targets: Vec<Target>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Target {
    pub(crate) kind: Vec<String>,
    pub(crate) src_path: PathBuf,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Resolve {
    pub(crate) nodes: Vec<Node>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Node {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) deps: Vec<NodeDep>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NodeDep {
    pub(crate) pkg: String,
}

impl Metadata {
    /// Parse the metadata document the generated step wrote.
    pub(crate) fn read(path: &Path) -> Result<Self, OrchestratorError> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| OrchestratorError::io(path.display().to_string(), err.to_string()))?;
        serde_json::from_str(&text).map_err(|_| internal("unparsable_seed_metadata"))
    }

    /// Packages without a registry or Git source.
    pub(crate) fn path_packages(&self) -> BTreeSet<&str> {
        self.packages
            .iter()
            .filter(|package| package.source.is_none())
            .map(|package| package.id.as_str())
            .collect()
    }

    /// Resolve edges `node -> deps`.
    pub(crate) fn edges(&self) -> BTreeMap<&str, Vec<&str>> {
        self.resolve
            .iter()
            .flat_map(|resolve| &resolve.nodes)
            .map(|node| {
                (
                    node.id.as_str(),
                    node.deps.iter().map(|dep| dep.pkg.as_str()).collect(),
                )
            })
            .collect()
    }

    /// Path packages a registry or Git package depends on, plus their
    /// path-dependency closure: their real sources stay.
    pub(crate) fn protected_path_packages(&self) -> BTreeSet<&str> {
        let path = self.path_packages();
        let edges = self.edges();
        let mut stack: Vec<&str> = edges
            .iter()
            .filter(|(node, _)| !path.contains(*node))
            .flat_map(|(_, deps)| deps.iter().copied())
            .filter(|dep| path.contains(dep))
            .collect();
        let mut protected = BTreeSet::new();
        while let Some(id) = stack.pop() {
            if protected.insert(id) {
                stack.extend(
                    edges
                        .get(id)
                        .into_iter()
                        .flatten()
                        .copied()
                        .filter(|dep| path.contains(dep)),
                );
            }
        }
        protected
    }
}

/// Stub the members of the root named by the runner environment.
/// # Errors
///
/// Returns an internal or IO error for missing context, unreadable
/// metadata, or a member source outside the workspace.
pub fn seed_skeleton() -> Result<String, OrchestratorError> {
    let workspace = super::absolute_env("GITHUB_WORKSPACE")?;
    let runner_temp = super::absolute_env("RUNNER_TEMP")?;
    let metadata = Metadata::read(&runner_temp.join(super::SEED_DIR).join("metadata.json"))?;
    let stubs = plan_stubs(&metadata, &workspace)?;
    for (path, stub) in &stubs {
        std::fs::write(path, stub)
            .map_err(|err| OrchestratorError::io(path.display().to_string(), err.to_string()))?;
    }
    Ok(format!(
        "velnor: seed skeleton stubbed {} files; kept {} patched path packages\n",
        stubs.len(),
        metadata.protected_path_packages().len()
    ))
}

/// Every member source file to rewrite and its stub, validated in-workspace.
/// # Errors
///
/// Returns an internal error for a source outside the workspace, a
/// symlinked source, or a member missing from the package list.
pub(crate) fn plan_stubs(
    metadata: &Metadata,
    workspace: &Path,
) -> Result<BTreeMap<PathBuf, &'static str>, OrchestratorError> {
    let protected = metadata.protected_path_packages();
    let workspace = std::fs::canonicalize(workspace)
        .map_err(|err| OrchestratorError::io(workspace.display().to_string(), err.to_string()))?;
    let mut kinds: BTreeMap<PathBuf, (bool, bool)> = BTreeMap::new();
    for member in &metadata.workspace_members {
        if protected.contains(member.as_str()) {
            continue;
        }
        let package = metadata
            .packages
            .iter()
            .find(|package| &package.id == member)
            .ok_or_else(|| internal("seed_member_without_package"))?;
        for target in &package.targets {
            let lib = target
                .kind
                .iter()
                .any(|kind| LIB_KINDS.contains(&kind.as_str()));
            let entry = kinds
                .entry(checked_source(&workspace, &target.src_path)?)
                .or_default();
            if lib {
                entry.0 = true;
            } else {
                entry.1 = true;
            }
        }
    }
    Ok(kinds
        .into_iter()
        .map(|(path, kind)| {
            let stub = match kind {
                (true, true) => SHARED_STUB,
                (true, false) => LIB_STUB,
                _ => BIN_STUB,
            };
            (path, stub)
        })
        .collect())
}

/// A regular, non-symlinked source file below the workspace.
fn checked_source(workspace: &Path, source: &Path) -> Result<PathBuf, OrchestratorError> {
    let meta = std::fs::symlink_metadata(source).map_err(|_| internal("seed_source_missing"))?;
    if !meta.is_file() {
        return Err(internal("seed_source_not_regular"));
    }
    let resolved = std::fs::canonicalize(source).map_err(|_| internal("seed_source_missing"))?;
    if resolved != source || !resolved.starts_with(workspace) {
        return Err(internal("seed_source_outside_workspace"));
    }
    Ok(resolved)
}

#[cfg(test)]
#[path = "seed_skeleton_tests.rs"]
mod tests;
