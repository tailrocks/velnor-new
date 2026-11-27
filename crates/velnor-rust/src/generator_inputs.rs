// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    CheckConfigurations, MemberDependency, PackageName, RepoFacts, RepoPath, RustCiConfig, Target,
    TargetKind,
};

use crate::detect_checks::{generator_root, is_generator};
use crate::select::relative;
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

    fn directories(&self) -> impl Iterator<Item = Option<RepoPath>> + '_ {
        let outside = self
            .targets
            .into_iter()
            .flatten()
            .filter_map(|target| target.source.as_ref())
            .filter(|source| relative(self.directory.as_ref(), source).is_none())
            .map(RepoPath::parent);
        std::iter::once(self.directory.clone()).chain(outside)
    }
}

fn nodes(facts: &RepoFacts) -> BTreeMap<&RepoPath, Node<'_>> {
    let members = facts
        .rust_roots
        .iter()
        .flat_map(|root| &root.members)
        .map(|member| {
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
    let unowned = facts.unowned.iter().map(|(manifest, package)| {
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

fn generator_manifest(facts: &RepoFacts) -> Option<&RepoPath> {
    generator_root(&facts.rust_roots)?
        .members
        .iter()
        .find(|member| is_generator(member))
        .map(|member| &member.manifest)
}

fn closure_of<'a>(
    facts: &RepoFacts,
    nodes: &'a BTreeMap<&RepoPath, Node<'a>>,
) -> Vec<&'a Node<'a>> {
    let mut pending: Vec<&RepoPath> = generator_manifest(facts).into_iter().collect();
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

fn owns_unresolved_input(facts: &RepoFacts, closure: &[&Node<'_>]) -> bool {
    facts
        .compile_inputs
        .iter()
        .filter(|(key, _)| key.is_none())
        .flat_map(|(_, owners)| owners)
        .any(|owner| {
            closure
                .iter()
                .any(|node| owner.is(node.name, node.manifest))
        })
}

pub(crate) struct Generator {
    sealed: bool,
    directories: Vec<Option<RepoPath>>,
    files: BTreeSet<RepoPath>,
    named: Vec<String>,
}

impl Generator {
    pub(crate) fn of(facts: &RepoFacts) -> Self {
        let nodes = nodes(facts);
        let closure = closure_of(facts, &nodes);
        let sealed = closure.is_empty()
            || closure.iter().any(|node| node.is_opaque())
            || owns_unresolved_input(facts, &closure);
        Self {
            sealed,
            directories: closure.iter().flat_map(|node| node.directories()).collect(),
            files: files(facts),
            named: named(facts),
        }
    }

    pub(crate) fn reads(&self, path: &RepoPath) -> bool {
        self.sealed
            || self
                .directories
                .iter()
                .any(|directory| contains(directory.as_ref(), path))
            || self.files.contains(path)
            || self.named.iter().any(|name| overlaps(name, path.as_str()))
    }
}

fn files(facts: &RepoFacts) -> BTreeSet<RepoPath> {
    let scripts = facts
        .rust_roots
        .iter()
        .flat_map(|root| &root.members)
        .flat_map(|member| &member.targets)
        .filter(|target| target.kinds.contains(&TargetKind::BuildScript))
        .filter_map(|target| target.source.clone());
    facts
        .compile_inputs
        .keys()
        .flatten()
        .cloned()
        .chain(scripts)
        .collect()
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
