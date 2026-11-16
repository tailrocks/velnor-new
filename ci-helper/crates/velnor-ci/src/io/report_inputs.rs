// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::inputs::parse_inputs;
use crate::logscan::named_crate;
use crate::report::{Compilation, LaneMetrics, SNAPSHOT};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use velnor_model::{AllTrigger, ChangeSet, RepoPath, RustSelection};
fn ci(error: impl std::fmt::Display) -> CiError {
    CiError::new(error.to_string())
}

pub(super) fn collect_inputs(metrics: &mut LaneMetrics, log: &[u8]) -> Result<(), CiError> {
    let checkout = velnor_gen::io::read_recorded_checkout(&super::current_dir()?).map_err(ci)?;
    let facts = velnor_rust::detect(&checkout.view).map_err(ci)?;
    let (_, current) = super::inputs::worktree_files(&checkout.root)?;
    for root in &facts.rust_roots {
        let directory = root.manifest.parent();
        let root_text = directory.as_ref().map_or(".", RepoPath::as_str);
        let snapshot = checkout
            .root
            .join(directory.as_ref().map_or("", RepoPath::as_str))
            .join(SNAPSHOT);
        let saved = fs::read_to_string(&snapshot)
            .ok()
            .map(|text| parse_inputs(&text))
            .transpose()?;
        let usable = saved.is_some();
        let same_lock = same_lock(&root.manifest, &current, saved.as_deref())?;
        let unchanged = unchanged(&facts, root, &current, saved);
        for cache in &mut metrics.caches {
            if cache.family == "target" && cache.root.as_deref() == Some(root_text) {
                cache.usable = usable;
                cache.usable_same_lock = usable && same_lock;
            }
        }
        for invocation in &mut metrics.invocations {
            let cwd = Path::new(&invocation.root);
            if cwd
                != checkout
                    .root
                    .join(directory.as_ref().map_or("", RepoPath::as_str))
            {
                continue;
            }
            root_text.clone_into(&mut invocation.root);
            let slice = log
                .get(invocation.log_start..invocation.log_end)
                .ok_or_else(|| CiError::new("invocation log range is invalid"))?;
            let slice = String::from_utf8_lossy(slice);
            invocation.downloads = slice
                .lines()
                .filter_map(named_crate)
                .filter(|(_, _, downloaded)| *downloaded)
                .map(|(name, version, _)| format!("{name} v{version}"))
                .collect();
            invocation.compilations = slice
                .lines()
                .filter(|line| {
                    !crate::logscan::strip_ansi(line)
                        .trim_start()
                        .starts_with("Documenting ")
                })
                .filter_map(|line| named_crate(line).map(|event| (line, event)))
                .filter(|(_, (_, _, downloaded))| !downloaded)
                .map(|(line, (package, version, _))| {
                    let member = root.members.iter().find(|member| {
                        member.name.as_str() == package
                            && line.contains(&format!(
                                "{package} v{version} ({})",
                                member
                                    .directory
                                    .as_ref()
                                    .map_or_else(
                                        || checkout.root.clone(),
                                        |directory| checkout.root.join(directory.as_str())
                                    )
                                    .display()
                            ))
                    });
                    Compilation {
                        unchanged: unchanged.contains(package.as_str()),
                        workspace: member.is_some(),
                        dependencies: dependencies(root, member),
                        package,
                        version,
                    }
                })
                .collect();
        }
    }
    Ok(())
}

fn same_lock(
    manifest: &RepoPath,
    current: &crate::inputs::Tracked,
    saved: Option<&[crate::inputs::SavedInput]>,
) -> Result<bool, CiError> {
    let lock = manifest.sibling(&RepoPath::new("Cargo.lock").map_err(ci)?);
    Ok(saved.is_some_and(|saved| {
        saved
            .iter()
            .find(|input| input.file.path == lock.as_str())
            .is_some_and(|input| current.files.contains(&input.file))
    }))
}

fn unchanged(
    facts: &velnor_model::RepoFacts,
    root: &velnor_model::RustRoot,
    current: &crate::inputs::Tracked,
    saved: Option<Vec<crate::inputs::SavedInput>>,
) -> BTreeSet<String> {
    let mut unchanged = BTreeSet::new();
    if current.unsupported > 0 || current.unverifiable_links > 0 || !current.absent.is_empty() {
        return unchanged;
    }
    let current: BTreeMap<_, _> = current
        .files
        .iter()
        .chain(&current.links)
        .map(|file| (&file.path, file))
        .collect();
    if let Some(saved) = saved {
        let saved: BTreeMap<_, _> = saved
            .iter()
            .map(|input| (&input.file.path, &input.file))
            .collect();
        let scope = crate::scope::Scope::new(
            root.manifest.parent().as_ref().map_or("", RepoPath::as_str),
            facts
                .compile_inputs
                .keys()
                .flatten()
                .map(ToString::to_string)
                .collect(),
        );
        let current: BTreeMap<_, _> = current
            .iter()
            .filter(|(path, _)| {
                scope.contains(path)
                    || RepoPath::new(path.as_str()).is_ok_and(|path| {
                        facts
                            .config
                            .inputs
                            .iter()
                            .any(|rule| rule.glob.matches(&path))
                    })
            })
            .map(|(path, file)| (*path, *file))
            .collect();
        let changes = ChangeSet {
            paths: saved
                .keys()
                .chain(current.keys())
                .filter(|path| saved.get(*path) != current.get(*path))
                .filter_map(|path| RepoPath::new(path.as_str()).ok())
                .collect(),
            ..ChangeSet::default()
        };
        let selection = velnor_rust::select(facts, &changes);
        for member in &root.members {
            if matches!(selection.get(&root.manifest), Some(RustSelection::None))
                || matches!(selection.get(&root.manifest), Some(RustSelection::Some(packages) | RustSelection::All(AllTrigger::MostPackages(packages))) if !packages.contains_key(&member.name))
            {
                unchanged.insert(member.name.to_string());
            }
        }
    }

    unchanged
}

fn dependencies(
    root: &velnor_model::RustRoot,
    member: Option<&velnor_model::RustMember>,
) -> BTreeSet<String> {
    let mut dependencies = BTreeSet::new();
    let mut pending: Vec<_> = member
        .into_iter()
        .flat_map(|member| &member.dependencies)
        .map(|dependency| &dependency.name)
        .collect();
    while let Some(name) = pending.pop() {
        if dependencies.insert(name.to_string())
            && let Some(member) = root.members.iter().find(|member| member.name == *name)
        {
            pending.extend(
                member
                    .dependencies
                    .iter()
                    .map(|dependency| &dependency.name),
            );
        }
    }
    dependencies
}
