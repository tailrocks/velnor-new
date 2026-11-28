// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    CheckConfigurations, InputKey, MemberDependency, PackageName, RepoFacts, RepoPath,
    RustCiConfig, RustRoot, Target, TargetKind, UnownedPackage,
};

use crate::audit_job::audited_on_pull_requests;
use crate::detect_checks::{generator_root, is_generator};
use crate::select_paths::contains;

struct Node<'a> {
    name: &'a PackageName,
    manifest: &'a RepoPath,
    directory: Option<RepoPath>,
    dependencies: &'a [MemberDependency],
    targets: Option<&'a [Target]>,
}

impl Node<'_> {
    fn is_opaque(&self) -> bool {
        self.targets.is_none_or(|targets| {
            targets.iter().any(|target| {
                target
                    .kinds
                    .iter()
                    .any(|kind| matches!(kind, TargetKind::BuildScript | TargetKind::ProcMacro))
            })
        })
    }
}

fn nodes<'a>(
    roots: &'a [RustRoot],
    unowned: &'a BTreeMap<RepoPath, UnownedPackage>,
) -> BTreeMap<&'a RepoPath, Node<'a>> {
    let members = roots.iter().flat_map(|root| &root.members).map(|member| {
        (
            &member.manifest,
            Node {
                name: &member.name,
                manifest: &member.manifest,
                directory: member.directory.clone(),
                dependencies: &member.dependencies,
                targets: Some(&member.targets),
            },
        )
    });
    let unowned = unowned.iter().map(|(manifest, package)| {
        (
            manifest,
            Node {
                name: &package.name,
                manifest,
                directory: manifest.parent(),
                dependencies: &package.dependencies,
                targets: None,
            },
        )
    });
    members.chain(unowned).collect()
}

fn generator_manifest(roots: &[RustRoot]) -> Option<&RepoPath> {
    generator_root(roots)?
        .members
        .iter()
        .find(|member| is_generator(member))
        .map(|member| &member.manifest)
}

fn generator_closure<'a>(
    roots: &[RustRoot],
    nodes: &'a BTreeMap<&RepoPath, Node<'a>>,
) -> Vec<&'a Node<'a>> {
    let mut pending: Vec<&RepoPath> = generator_manifest(roots).into_iter().collect();
    let mut seen: BTreeSet<&RepoPath> = BTreeSet::new();
    while let Some(manifest) = pending.pop() {
        if !seen.insert(manifest) {
            continue;
        }
        if let Some(node) = nodes.get(manifest) {
            pending.extend(
                node.dependencies
                    .iter()
                    .map(|dependency| &dependency.manifest),
            );
        }
    }
    seen.iter()
        .filter_map(|manifest| nodes.get(*manifest))
        .collect()
}

pub(crate) fn closure_manifests(
    roots: &[RustRoot],
    unowned: &BTreeMap<RepoPath, UnownedPackage>,
) -> Vec<RepoPath> {
    let nodes = nodes(roots, unowned);
    generator_closure(roots, &nodes)
        .into_iter()
        .map(|node| node.manifest.clone())
        .collect()
}

pub(crate) struct Generator {
    sealed: bool,
    directories: Vec<Option<RepoPath>>,
    locked: Vec<RepoPath>,
    named: Vec<String>,
}

impl Generator {
    pub(crate) fn of(facts: &RepoFacts) -> Self {
        let nodes = nodes(&facts.rust_roots, &facts.unowned);
        let closure = generator_closure(&facts.rust_roots, &nodes);
        let sealed = closure.is_empty()
            || facts.generator_unmodelled
            || !facts.inputs_lock_identified
            || !audited_on_pull_requests(facts.helper.as_ref(), &facts.config)
            || closure.iter().any(|node| node.is_opaque())
            || has_pathless_line(facts, &closure);
        Self {
            sealed,
            directories: closure.iter().map(|node| node.directory.clone()).collect(),
            locked: locked(facts, &closure),
            named: named(facts),
        }
    }

    pub(crate) fn reads(&self, path: &RepoPath) -> bool {
        self.sealed
            || self
                .directories
                .iter()
                .any(|directory| contains(directory.as_ref(), path))
            || self
                .locked
                .iter()
                .any(|entry| entry == path || contains(Some(entry), path))
            || self.named.iter().any(|name| overlaps(name, path.as_str()))
    }
}

fn closure_lines<'a>(
    facts: &'a RepoFacts,
    closure: &'a [&Node<'_>],
) -> impl Iterator<Item = &'a InputKey> {
    facts
        .compile_inputs
        .iter()
        .filter(|(_, owners)| {
            owners.iter().any(|owner| {
                closure
                    .iter()
                    .any(|node| owner.is(node.name, node.manifest))
            })
        })
        .map(|(key, _)| key)
}

fn locked(facts: &RepoFacts, closure: &[&Node<'_>]) -> Vec<RepoPath> {
    closure_lines(facts, closure)
        .filter_map(|key| key.path().cloned())
        .collect()
}

fn has_pathless_line(facts: &RepoFacts, closure: &[&Node<'_>]) -> bool {
    closure_lines(facts, closure).any(|key| key.path().is_none())
}

fn named(facts: &RepoFacts) -> Vec<String> {
    let config: &RustCiConfig = &facts.config;
    let images = config.images.iter().flat_map(|image| {
        let stage = if image.context == "." {
            image.stage.clone()
        } else {
            format!("{}/{}", image.context, image.stage)
        };
        let ignore = if image.context == "." {
            ".dockerignore".to_owned()
        } else {
            format!("{}/.dockerignore", image.context)
        };
        [
            image.dockerfile.clone(),
            format!("{}.dockerignore", image.dockerfile),
            ignore,
            stage,
        ]
        .into_iter()
        .chain(image.inputs.clone())
    });
    let targets = facts
        .rust_roots
        .iter()
        .map(|root| (root.target_dir.to_string(), delta(&root.target_dir)))
        .chain(
            config
                .workspace_groups
                .iter()
                .map(|group| (group.target_dir().to_string(), delta(&group.target_dir()))),
        )
        .flat_map(|(target, delta)| [target, delta]);
    let configurations = config
        .check_configurations
        .iter()
        .filter_map(CheckConfigurations::file)
        .map(|file| file.to_string());
    images.chain(targets).chain(configurations).collect()
}

fn delta(target: &RepoPath) -> String {
    format!("{target}.velnor-delta")
}

fn overlaps(name: &str, path: &str) -> bool {
    name == path || path.starts_with(&format!("{name}/")) || name.starts_with(&format!("{path}/"))
}
