// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use velnor_model::{ChangeSet, RepoFacts, RepoPath};

use super::delta::{failure, safe_path};
use super::delta_source::SourceSnapshot;
use crate::error::CiError;

pub(super) fn invalidate(target: &Path, saved: &SourceSnapshot) -> Result<usize, CiError> {
    let directory = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let root = fs::canonicalize(directory).map_err(|e| failure(directory, &e))?;
    let text =
        fs::read_to_string(target.join(".velnor/inputs.tsv")).map_err(|e| failure(target, &e))?;
    if super::input_snapshot::decode(&text)?.source_identity != saved.identity {
        return Err(CiError::new(
            "delta sources differ from the saved input identity",
        ));
    }
    let before = saved.records()?;
    let tracked = super::inputs::source_files(&root)?;
    let current = SourceSnapshot {
        identity: super::delta_source::identity_of(&tracked)?,
        tracked,
    };
    let after = current.records()?;
    let paths = before
        .keys()
        .chain(after.keys())
        .filter(|path| before.get(*path) != after.get(*path))
        .cloned()
        .collect::<BTreeSet<_>>();
    if paths.is_empty() {
        return Ok(0);
    }
    let fingerprints = fingerprints(target)?;
    if fingerprints.is_empty() {
        return Ok(0);
    }
    let checkout =
        velnor_gen::io::read_recorded_checkout(&root).map_err(|e| CiError::new(e.to_string()))?;
    let facts = velnor_rust::detect(&checkout.view).map_err(|e| CiError::new(e.to_string()))?;
    let changes = ChangeSet {
        paths,
        ..ChangeSet::default()
    };
    let affected = velnor_rust::affected_local_packages(&facts, &changes)
        .ok_or_else(|| CiError::new("source changes require a cold build"))?;
    let manifest = root
        .join("Cargo.toml")
        .strip_prefix(&checkout.root)
        .ok()
        .and_then(|path| path.to_str())
        .and_then(|path| RepoPath::new(path).ok())
        .ok_or_else(|| CiError::new("cannot place the restored workspace"))?;
    let relevant = reachable(&facts, &manifest)?;
    let names = affected
        .iter()
        .filter(|(path, _)| relevant.contains(*path))
        .map(|(_, name)| name.as_str())
        .collect::<BTreeSet<_>>();
    let mut removed = 0;
    for (path, name) in fingerprints {
        if names.contains(name.as_str()) {
            safe_path(&path)?;
            super::delta_tree::remove_node(&path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn fingerprints(target: &Path) -> Result<BTreeMap<PathBuf, String>, CiError> {
    let mut result = BTreeMap::new();
    for path in super::delta_tree::paths(target)? {
        if path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == ".fingerprint")
            && let Some((name, _)) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(super::units::split)
        {
            let full = target.join(&path);
            if fs::symlink_metadata(&full)
                .map_err(|e| failure(&full, &e))?
                .is_dir()
            {
                result.insert(full, name.to_owned());
            }
        }
    }
    Ok(result)
}

fn reachable(facts: &RepoFacts, manifest: &RepoPath) -> Result<BTreeSet<RepoPath>, CiError> {
    let invalid = || CiError::new("restored workspace dependency ownership is unavailable");
    let root = facts
        .rust_roots
        .iter()
        .find(|root| root.manifest == *manifest)
        .ok_or_else(invalid)?;
    let mut pending = root
        .members
        .iter()
        .map(|member| member.manifest.clone())
        .collect::<Vec<_>>();
    let mut reached = BTreeSet::new();
    while let Some(manifest) = pending.pop() {
        if !reached.insert(manifest.clone()) {
            continue;
        }
        let dependencies = facts
            .rust_roots
            .iter()
            .flat_map(|root| &root.members)
            .find(|member| member.manifest == manifest)
            .map(|member| &member.dependencies)
            .or_else(|| {
                facts
                    .unowned
                    .get(&manifest)
                    .filter(|package| package.unread.is_none())
                    .map(|package| &package.dependencies)
            })
            .ok_or_else(invalid)?;
        pending.extend(
            dependencies
                .iter()
                .map(|dependency| dependency.manifest.clone()),
        );
    }
    Ok(reached)
}
