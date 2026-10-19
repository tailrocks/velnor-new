//! Path ownership and reverse-closure selection over the package graph.
//!
//! A changed path is claimed by every package that can read it:
//!
//! - the deepest package directory containing it (workspace members and
//!   in-repository path packages from the full resolve, so a vendored or
//!   `[patch]`ed crate owns its own directory);
//! - every member below a directory-scoped Cargo tool config
//!   (`clippy.toml`, `rustfmt.toml`, `.cargo/config*` and their dotted
//!   variants), because those tools search upward from each member;
//! - every member of a workspace whose root `Cargo.toml` or
//!   `.config/nextest.toml` changed;
//! - every package named by a matching `[[stacks.rust.inputs]]` rule;
//! - the owners of every tracked symlink whose resolved target (or any
//!   hop on the way) contains it.
//!
//! Claims are a union: a file inside package X's directory that is also
//! declared as an input of package Y selects both. A path no rule claims
//! is unowned, and the caller then selects everything.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::reverse_closure;

use crate::discover::Discovery;
use crate::select_symlinks::ResolvedLink;

/// Basenames whose tool searches upward from each member directory.
const UPWARD_CONFIG_BASENAMES: &[&str] = &[
    "clippy.toml",
    ".clippy.toml",
    "rustfmt.toml",
    ".rustfmt.toml",
];

/// Path-to-package claims for one discovery plus its resolved symlinks.
pub(crate) struct Ownership<'a> {
    /// `(dir, package id)` for members and resolved path packages.
    owners: Vec<(String, String)>,
    /// `(dir, package id)` for workspace members only.
    members: Vec<(String, String)>,
    /// `(workspace root, member ids)` per planned workspace.
    workspaces: Vec<(String, Vec<String>)>,
    /// Discovery whose declared input rules apply.
    discovery: &'a Discovery,
    /// Resolved tracked symlinks with the packages that own each link.
    links: Vec<(ResolvedLink, BTreeSet<String>)>,
    /// Package name per workspace-member ID, for stable warnings.
    names: BTreeMap<String, String>,
}

impl<'a> Ownership<'a> {
    /// Ownership over discovery packages plus resolved path packages.
    pub(crate) fn new(discovery: &'a Discovery) -> Self {
        let mut owners = Vec::new();
        let mut members = Vec::new();
        let mut workspaces = Vec::new();
        let mut names = BTreeMap::new();
        for workspace in &discovery.workspaces {
            let mut ids = Vec::new();
            for package in &workspace.record.packages {
                if package.in_workspace && !package.external {
                    let dir = manifest_dir(&package.manifest);
                    owners.push((dir.clone(), package.id.clone()));
                    members.push((dir, package.id.clone()));
                    ids.push(package.id.clone());
                    names.insert(package.id.clone(), package.name.clone());
                }
            }
            workspaces.push((workspace.record.workspace_root.clone(), ids));
        }
        for resolve in &discovery.affected.resolves {
            if let Ok(graph) = &resolve.graph {
                for package in &graph.path_packages {
                    if !owners.iter().any(|(_, id)| *id == package.id) {
                        owners.push((package.dir.clone(), package.id.clone()));
                    }
                }
            }
        }
        Self {
            owners,
            members,
            workspaces,
            discovery,
            links: Vec::new(),
            names,
        }
    }

    /// The deepest package owning `path`, with its directory.
    pub(crate) fn package_of(&self, path: &str) -> Option<&(String, String)> {
        deepest(&self.owners, path)
    }

    /// Stable display name for a package ID: its Cargo name, else its directory.
    pub(crate) fn name_of(&self, id: &str) -> String {
        self.names.get(id).cloned().unwrap_or_else(|| {
            self.owners
                .iter()
                .find(|(_, owner)| owner == id)
                .map_or_else(|| id.to_owned(), |(dir, _)| dir.clone())
        })
    }

    /// True when some declared input rule names package `id`.
    pub(crate) fn declares_inputs(&self, id: &str) -> bool {
        self.discovery
            .affected
            .rules
            .iter()
            .any(|rule| rule.package_ids.contains(id))
    }

    /// Attach resolved links; each link's owners come from the other rules.
    pub(crate) fn with_links(mut self, links: Vec<ResolvedLink>) -> Self {
        let mut owned = Vec::with_capacity(links.len());
        for link in links {
            let claim = self.claim(&link.link);
            if !claim.is_empty() {
                owned.push((link, claim));
            }
        }
        self.links = owned;
        self
    }

    /// True when some package other than a root package claims `path`.
    ///
    /// The root package (empty directory) owns every path by prefix, so
    /// counting it would narrow stray files to the root instead of
    /// broadening. With no nested package every path is trivially
    /// classified: broadening and narrowing select the same package.
    pub(crate) fn is_owned(&self, path: &str) -> bool {
        if self.owners.iter().all(|(dir, _)| dir.is_empty()) {
            return true;
        }
        self.owners
            .iter()
            .any(|(dir, _)| !dir.is_empty() && owns(dir, path))
            || !self.claim_without_owners(path).is_empty()
    }

    /// Every package claiming `path`, root package included.
    pub(crate) fn claim(&self, path: &str) -> BTreeSet<String> {
        let mut claim = self.claim_without_owners(path);
        if let Some(id) = deepest_owner(&self.owners, path) {
            claim.insert(id);
        }
        claim
    }

    /// Claims from scoped configs, workspace roots, rules, and links.
    fn claim_without_owners(&self, path: &str) -> BTreeSet<String> {
        let mut claim = BTreeSet::new();
        if let Some(scope) = upward_config_scope(path) {
            for (dir, id) in &self.members {
                if scope.is_empty() || owns(scope, dir) {
                    claim.insert(id.clone());
                }
            }
        }
        for (root, ids) in &self.workspaces {
            if is_workspace_scoped(root, path) {
                claim.extend(ids.iter().cloned());
            }
        }
        for rule in &self.discovery.affected.rules {
            if rule.matches(path) {
                claim.extend(rule.package_ids.iter().cloned());
            }
        }
        for (link, owners) in &self.links {
            if link.reads(path) {
                claim.extend(owners.iter().cloned());
            }
        }
        claim
    }
}

/// Packages claiming changed files, plus forced packages, plus their reverse closure.
///
/// Forced packages (fail-safe signals: unverified reads) seed the closure
/// exactly like changed ones, so their dependents run too. The closure
/// runs over every supplied graph (base and head path edges plus the
/// resolved graph), so edges removed at head and edges through registry
/// crates still select their consumers.
pub(crate) fn affected_packages(
    ownership: &Ownership<'_>,
    changed: &BTreeSet<String>,
    forced: &BTreeSet<String>,
    graphs: &[&[(String, String)]],
) -> BTreeSet<String> {
    let mut owned = forced.clone();
    for path in changed {
        owned.extend(ownership.claim(path));
    }
    let edges: Vec<(String, String)> = graphs
        .iter()
        .flat_map(|graph| graph.iter().cloned())
        .collect();
    reverse_closure(&edges, &[], &owned)
}

/// True when any changed file has no claiming package.
pub(crate) fn has_unowned_file(ownership: &Ownership<'_>, changed: &BTreeSet<String>) -> bool {
    changed.iter().any(|path| !ownership.is_owned(path))
}

/// Directory whose descendants a scoped Cargo tool config applies to.
fn upward_config_scope(path: &str) -> Option<&str> {
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    if UPWARD_CONFIG_BASENAMES.contains(&name) {
        return Some(dir);
    }
    if name == "config" || name == "config.toml" {
        let (parent, cargo) = dir.rsplit_once('/').unwrap_or(("", dir));
        if cargo == ".cargo" {
            return Some(parent);
        }
    }
    None
}

/// True for a workspace-root manifest or Nextest config.
fn is_workspace_scoped(root: &str, path: &str) -> bool {
    let prefix = if root.is_empty() {
        String::new()
    } else {
        format!("{root}/")
    };
    path.strip_prefix(&prefix)
        .is_some_and(|rest| rest == "Cargo.toml" || rest == ".config/nextest.toml")
}

/// True when manifest directory `dir` owns `path`.
fn owns(dir: &str, path: &str) -> bool {
    dir.is_empty() || *path == *dir || path.starts_with(&format!("{dir}/"))
}

/// Deepest manifest directory owning `path`; the root package owns the rest.
fn deepest_owner(owners: &[(String, String)], path: &str) -> Option<String> {
    deepest(owners, path).map(|owner| owner.1.clone())
}

/// Deepest `(dir, id)` owning `path`.
fn deepest<'o>(owners: &'o [(String, String)], path: &str) -> Option<&'o (String, String)> {
    let mut best: Option<&(String, String)> = None;
    for owner in owners {
        if owns(&owner.0, path) && best.is_none_or(|current| owner.0.len() > current.0.len()) {
            best = Some(owner);
        }
    }
    best
}

/// Directory of a manifest path; empty for the repository root.
pub(crate) fn manifest_dir(manifest: &str) -> String {
    manifest
        .rsplit_once('/')
        .map_or_else(String::new, |(dir, _)| dir.to_owned())
}

#[cfg(test)]
#[path = "select_affected_tests.rs"]
mod tests;
