// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::reverse_closure;

use crate::discover::Discovery;
use crate::select_symlinks::ResolvedLink;

const UPWARD_CONFIG_BASENAMES: &[&str] = &[
    "clippy.toml",
    ".clippy.toml",
    "rustfmt.toml",
    ".rustfmt.toml",
];

pub(crate) struct Ownership<'a> {
    owners: Vec<(String, String)>,
    members: Vec<(String, String)>,
    workspaces: Vec<(String, Vec<String>)>,
    discovery: &'a Discovery,
    links: Vec<(ResolvedLink, BTreeSet<String>)>,
    names: BTreeMap<String, String>,
}

impl<'a> Ownership<'a> {
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

    pub(crate) fn package_of(&self, path: &str) -> Option<&(String, String)> {
        deepest(&self.owners, path)
    }

    pub(crate) fn name_of(&self, id: &str) -> String {
        self.names.get(id).cloned().unwrap_or_else(|| {
            self.owners
                .iter()
                .find(|(_, owner)| owner == id)
                .map_or_else(|| id.to_owned(), |(dir, _)| dir.clone())
        })
    }

    pub(crate) fn has_claims_below(&self, dir: &str) -> bool {
        let prefix = format!("{dir}/");
        let below = |path: &str| dir.is_empty() || path.starts_with(&prefix);
        dir.is_empty()
            || self.owners.iter().any(|(owner, _)| below(owner))
            || self.workspaces.iter().any(|(root, _)| below(root))
            || self.discovery.affected.rules.iter().any(|rule| {
                rule.globs
                    .iter()
                    .any(|glob| below(glob) || glob.starts_with(['*', '?', '[', '{']))
            })
    }

    pub(crate) fn declares_complete_inputs(&self, id: &str) -> bool {
        self.discovery
            .affected
            .rules
            .iter()
            .any(|rule| rule.complete && rule.package_ids.contains(id))
    }

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

    pub(crate) fn is_owned(&self, path: &str) -> bool {
        if self.owners.iter().all(|(dir, _)| dir.is_empty()) {
            return true;
        }
        self.owners
            .iter()
            .any(|(dir, _)| !dir.is_empty() && owns(dir, path))
            || !self.claim_without_owners(path).is_empty()
    }

    pub(crate) fn claim(&self, path: &str) -> BTreeSet<String> {
        let mut claim = self.claim_without_owners(path);
        if let Some(id) = deepest_owner(&self.owners, path) {
            claim.insert(id);
        }
        claim
    }

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

pub(crate) fn has_unowned_file(ownership: &Ownership<'_>, changed: &BTreeSet<String>) -> bool {
    changed.iter().any(|path| !ownership.is_owned(path))
}

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

fn is_workspace_scoped(root: &str, path: &str) -> bool {
    let prefix = if root.is_empty() {
        String::new()
    } else {
        format!("{root}/")
    };
    path.strip_prefix(&prefix)
        .is_some_and(|rest| rest == "Cargo.toml" || rest == ".config/nextest.toml")
}

fn owns(dir: &str, path: &str) -> bool {
    dir.is_empty() || *path == *dir || path.starts_with(&format!("{dir}/"))
}

fn deepest_owner(owners: &[(String, String)], path: &str) -> Option<String> {
    deepest(owners, path).map(|owner| owner.1.clone())
}

fn deepest<'o>(owners: &'o [(String, String)], path: &str) -> Option<&'o (String, String)> {
    let mut best: Option<&(String, String)> = None;
    for owner in owners {
        if owns(&owner.0, path) && best.is_none_or(|current| owner.0.len() > current.0.len()) {
            best = Some(owner);
        }
    }
    best
}

pub(crate) fn manifest_dir(manifest: &str) -> String {
    manifest
        .rsplit_once('/')
        .map_or_else(String::new, |(dir, _)| dir.to_owned())
}
