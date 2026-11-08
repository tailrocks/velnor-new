// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Duration,
};

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

    pub(crate) fn owner_dirs(&self) -> impl Iterator<Item = &str> {
        self.owners.iter().map(|(dir, _)| dir.as_str())
    }

    pub(crate) fn is_tool_config(&self, path: &str) -> bool {
        upward_config_scope(path).is_some()
            || self
                .workspaces
                .iter()
                .any(|(root, _)| is_workspace_scoped(root, path))
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

pub(crate) fn scoped_standalone_workflow(
    discovery: &Discovery,
    changed: &BTreeSet<String>,
    root: &Path,
    base: &str,
    head: &str,
) -> Option<&'static str> {
    let path =
        velnor_actions_workflow_renderer::schema2::StandaloneWorkflow::JobImageCandidate.path();
    if !discovery.affected.velnor_repository
        || !changed.contains(path)
        || changed
            .iter()
            .any(|file| file == "Cargo.toml" || file.ends_with("/Cargo.toml"))
        || changed.iter().any(|file| {
            file != path
                && (crate::decisions::selection_broadens_for_path(file).is_some()
                    || crate::decisions::broadening_for_path(file).is_some())
        })
    {
        return None;
    }
    if !has_complete_workflow_owners(discovery, path) {
        return None;
    }
    for directory in [root.join(".github"), root.join(".github/workflows")] {
        let metadata = std::fs::symlink_metadata(directory).ok()?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return None;
        }
    }
    let metadata = std::fs::symlink_metadata(root.join(path)).ok()?;
    if !metadata.file_type().is_file() {
        return None;
    }
    let git_dir = root.join(".git");
    let objects = git_dir.join("objects");
    for directory in [&git_dir, &objects] {
        let metadata = std::fs::symlink_metadata(directory).ok()?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return None;
        }
    }
    match std::fs::symlink_metadata(objects.join("info/alternates")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => return None,
    }
    for revision in [base, head] {
        regular_tree_entry(root, &git_dir, &objects, revision, path)?;
    }
    Some(path)
}

fn has_complete_workflow_owners(discovery: &Discovery, path: &str) -> bool {
    let matching: Vec<_> = discovery
        .affected
        .rules
        .iter()
        .filter(|rule| rule.matches(path))
        .collect();
    if matching.is_empty() || matching.iter().any(|rule| !rule.complete) {
        return false;
    }
    let claimed: BTreeSet<_> = matching
        .iter()
        .flat_map(|rule| rule.package_ids.iter())
        .collect();
    for name in ["velnor-actions-workflow-renderer", "velnor-archive-guard"] {
        if !discovery
            .workspaces
            .iter()
            .flat_map(|workspace| &workspace.record.packages)
            .any(|package| {
                package.name == name
                    && package.in_workspace
                    && !package.external
                    && claimed.contains(&package.id)
            })
        {
            return false;
        }
    }
    true
}

fn regular_tree_entry(
    root: &Path,
    git_dir: &Path,
    objects: &Path,
    revision: &str,
    path: &str,
) -> Option<()> {
    let request = velnor_actions_mise::GitRequest::ls_tree_exact(revision, path).ok()?;
    let environment = [
        ("GIT_NO_REPLACE_OBJECTS".into(), "1".into()),
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
        ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
        ("GIT_CONFIG_COUNT".into(), "0".into()),
        ("GIT_CONFIG_PARAMETERS".into(), "".into()),
        ("GIT_DIR".into(), git_dir.as_os_str().into()),
        ("GIT_COMMON_DIR".into(), git_dir.as_os_str().into()),
        ("GIT_OBJECT_DIRECTORY".into(), objects.as_os_str().into()),
        ("GIT_ALTERNATE_OBJECT_DIRECTORIES".into(), "".into()),
        ("GIT_NO_LAZY_FETCH".into(), "1".into()),
        ("GIT_TERMINAL_PROMPT".into(), "0".into()),
    ];
    let output = request
        .command_in(root)
        .with_env(&environment)
        .ok()?
        .run_bounded(4096, Duration::from_secs(2))
        .ok()?;
    let output = output.require_success("git").ok()?;
    let separator = output.stdout.iter().position(|byte| *byte == b'\t')?;
    let (header, suffix) = output.stdout.split_at(separator);
    let entry_path = suffix.get(1..)?;
    let parts: Vec<_> = header.split(|byte| *byte == b' ').collect();
    if parts.len() != 3
        || parts[0] != b"100644"
        || parts[1] != b"blob"
        || parts[2].len() != 40
        || !parts[2].iter().all(u8::is_ascii_hexdigit)
        || entry_path.strip_suffix(&[0])? != path.as_bytes()
    {
        return None;
    }
    Some(())
}
