// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    MemberDependency, PackageName, RepoPath, RepoView, RustCiConfig, RustMember, RustMetadata,
    RustPackage, RustRoot,
};

use crate::config::excluded;
use crate::error::{DetectError, Reason};
use crate::toml_file::table;

pub(crate) const CARGO_LOCK: RepoPath = RepoPath::from_static("Cargo.lock");

type Candidate<'a> = (&'a RepoPath, &'a RustMetadata);
type Targets = BTreeMap<PackageName, BTreeSet<RepoPath>>;

pub(crate) fn rust_roots(
    view: &RepoView,
    config: &RustCiConfig,
) -> Result<Vec<RustRoot>, DetectError> {
    let candidates: Vec<Candidate<'_>> = view
        .rust_metadata
        .iter()
        .filter(|(manifest, metadata)| !metadata.members.is_empty() && !excluded(config, manifest))
        .collect();
    if candidates.is_empty() {
        return Err(DetectError::NoRustRoot);
    }
    let owner: BTreeMap<&RepoPath, &RepoPath> = candidates
        .iter()
        .flat_map(|(root, metadata)| metadata.members.iter().map(|m| (&m.manifest, *root)))
        .collect();
    let mut named: Targets = BTreeMap::new();
    for package in candidates
        .iter()
        .flat_map(|(_, metadata)| &metadata.members)
    {
        named
            .entry(package.name.clone())
            .or_default()
            .insert(package.manifest.clone());
    }
    candidates
        .iter()
        .map(|(manifest, metadata)| {
            let lockfile = manifest.sibling(&CARGO_LOCK);
            if !view.tracked_paths.contains(&lockfile) {
                return Err(DetectError::MissingLockfile {
                    path: lockfile,
                    manifest: (*manifest).clone(),
                });
            }
            let mut targets = named.clone();
            for (file, bytes, base) in patch_files(view, manifest) {
                if file != *manifest {
                    rustdocflags_list(bytes, file)?;
                }
                for (name, target) in patches(bytes, file, base.as_ref())? {
                    targets.entry(name).or_default().insert(target);
                }
            }
            Ok(root(manifest, metadata, &owner, &targets))
        })
        .collect()
}

fn root(
    manifest: &RepoPath,
    metadata: &RustMetadata,
    owner: &BTreeMap<&RepoPath, &RepoPath>,
    targets: &Targets,
) -> RustRoot {
    let depends_on: BTreeSet<RepoPath> = metadata
        .members
        .iter()
        .flat_map(|package| &package.dependencies)
        .filter_map(|dependency| owner.get(dependency.manifest.as_ref()?))
        .filter(|root| **root != manifest)
        .map(|root| (*root).clone())
        .collect();
    let mut members: Vec<RustMember> = metadata
        .members
        .iter()
        .map(|package| member(package, targets))
        .collect();
    members.sort_by(|a, b| a.name.cmp(&b.name));
    RustRoot {
        manifest: manifest.clone(),
        members,
        depends_on: depends_on.into_iter().collect(),
    }
}

const CARGO_CONFIGS: [&str; 2] = [".cargo/config.toml", ".cargo/config"];

pub(crate) fn is_cargo_config(path: &RepoPath) -> bool {
    CARGO_CONFIGS.iter().any(|name| {
        path.as_str() == *name
            || path
                .as_str()
                .strip_suffix(name)
                .is_some_and(|rest| rest.ends_with('/'))
    })
}

fn patch_files<'v>(
    view: &'v RepoView,
    manifest: &RepoPath,
) -> Vec<(&'v RepoPath, &'v [u8], Option<RepoPath>)> {
    let folder = manifest.parent();
    view.files
        .iter()
        .filter_map(|(file, bytes)| {
            if file == manifest {
                return Some((file, bytes.as_slice(), folder.clone()));
            }
            if !is_cargo_config(file) {
                return None;
            }
            let base = file.parent().and_then(|cargo| cargo.parent());
            let applies = match (&base, &folder) {
                (None, _) => true,
                (Some(_), None) => false,
                (Some(base), Some(folder)) => {
                    folder == base || folder.as_str().starts_with(&format!("{base}/"))
                }
            };
            applies.then_some((file, bytes.as_slice(), base))
        })
        .collect()
}

fn rustdocflags_list(bytes: &[u8], file: &RepoPath) -> Result<(), DetectError> {
    let table = table(bytes).map_err(|reason| DetectError::invalid(file, reason))?;
    let flags = table
        .get("build")
        .and_then(|build| build.get("rustdocflags"));
    let per_target = table
        .get("target")
        .and_then(toml::Value::as_table)
        .is_some_and(|targets| {
            targets
                .values()
                .any(|target| target.get("rustdocflags").is_some())
        });
    if flags.is_some_and(toml::Value::is_str) || per_target {
        return Err(DetectError::invalid(file, Reason::RustdocflagsString));
    }
    if table.contains_key("include") {
        return Err(DetectError::invalid(file, Reason::CargoConfigInclude));
    }
    Ok(())
}

fn patches(
    bytes: &[u8],
    manifest: &RepoPath,
    base: Option<&RepoPath>,
) -> Result<Vec<(PackageName, RepoPath)>, DetectError> {
    let invalid = |key: String| DetectError::invalid(manifest, Reason::InvalidPatch(key));
    let table = table(bytes).map_err(|reason| DetectError::invalid(manifest, reason))?;
    let Some(patch) = table.get("patch") else {
        return Ok(Vec::new());
    };
    let sources = patch
        .as_table()
        .ok_or_else(|| invalid("patch".to_owned()))?;
    let mut found = Vec::new();
    for (source, entries) in sources {
        let entries = entries
            .as_table()
            .ok_or_else(|| invalid(format!("patch.{source}")))?;
        for (name, entry) in entries {
            let key = format!("patch.{source}.{name}");
            let entry = entry.as_table().ok_or_else(|| invalid(key.clone()))?;
            let Some(folder) = entry.get("path") else {
                continue;
            };
            let folder = folder.as_str().ok_or_else(|| invalid(key.clone()))?;
            let package = match entry.get("package") {
                Some(package) => package.as_str().ok_or_else(|| invalid(key.clone()))?,
                None => name.as_str(),
            };
            let name = PackageName::new(package).map_err(|_| invalid(key.clone()))?;
            if let Some(target) = within(base, folder) {
                found.push((name, RepoPath::new(target).map_err(|_| invalid(key))?));
            }
        }
    }
    Ok(found)
}

fn within(base: Option<&RepoPath>, relative: &str) -> Option<String> {
    let mut segments: Vec<&str> =
        base.map_or_else(Vec::new, |base| base.as_str().split('/').collect());
    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    segments.push("Cargo.toml");
    Some(segments.join("/"))
}

fn member(package: &RustPackage, targets: &Targets) -> RustMember {
    RustMember {
        name: package.name.clone(),
        manifest: package.manifest.clone(),
        directory: package.manifest.parent(),
        dependencies: package
            .dependencies
            .iter()
            .flat_map(|dependency| {
                let manifests: BTreeSet<RepoPath> = match &dependency.manifest {
                    Some(manifest) => BTreeSet::from([manifest.clone()]),
                    None => targets.get(&dependency.name).cloned().unwrap_or_default(),
                };
                manifests.into_iter().map(|manifest| MemberDependency {
                    name: dependency.name.clone(),
                    kind: dependency.kind,
                    manifest,
                })
            })
            .collect(),
        targets: package.targets.clone(),
    }
}
