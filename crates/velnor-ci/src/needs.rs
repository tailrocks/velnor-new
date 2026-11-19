// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    DependencyKind, MemberDependency, PackageName, RepoPath, RustMember, RustRoot, Target,
    TargetKind, UnownedPackage,
};

use crate::coverage::{UnitKind, crate_name};
use crate::platform::Platforms;

pub(crate) enum Need<'f> {
    Unit {
        host: bool,
        kind: UnitKind,
        package: &'f PackageName,
        target: String,
        source: &'f RepoPath,
    },
    Library {
        name: &'f PackageName,
        required: bool,
    },
    Run(&'f PackageName),
}

pub(crate) type Paths<'f> = BTreeMap<&'f PackageName, Vec<&'f RepoPath>>;

pub(crate) struct Group<'f> {
    pub(crate) label: String,
    pub(crate) needs: Vec<Need<'f>>,
}

pub(crate) fn groups<'f>(
    root: &'f RustRoot,
    paths: &'f Paths<'f>,
    reached: &BTreeMap<&'f RepoPath, &'f UnownedPackage>,
    platforms: &Platforms,
) -> Vec<Group<'f>> {
    let hosted = hosted(root, reached, platforms);
    let required = required(root, reached, platforms);
    let mut groups: Vec<Group<'f>> = root
        .members
        .iter()
        .flat_map(|member| {
            let hosted = hosted.contains(&member.manifest);
            member
                .targets
                .iter()
                .filter(|target| !target.requires_features)
                .filter_map(move |target| {
                    let source = target.source.as_ref()?;
                    Some(Group {
                        label: format!(
                            "{} ({}) target {} ({source}",
                            member.name, member.manifest, target.name
                        ),
                        needs: needs(&member.name, target, source, hosted),
                    })
                })
        })
        .collect();
    groups.extend(paths.iter().map(|(name, manifests)| {
        let manifests: Vec<String> = manifests.iter().map(ToString::to_string).collect();
        Group {
            label: format!("path dependency {name} ({}", manifests.join(" or ")),
            needs: vec![
                Need::Library {
                    name,
                    required: manifests.iter().any(|m| required.contains(m.as_str())),
                },
                Need::Run(name),
            ],
        }
    }));
    groups
}

fn required<'f>(
    root: &'f RustRoot,
    reached: &BTreeMap<&'f RepoPath, &'f UnownedPackage>,
    platforms: &Platforms,
) -> BTreeSet<&'f str> {
    // A dependency that holds only for a cross-built target (for example
    // `cfg(windows)` on a Linux host) is still compiled for that target, so it
    // is required too.
    let firm = |dependency: &MemberDependency| {
        !dependency.optional && applies_to_target_build(dependency, platforms)
    };
    let mut walk: Vec<&RepoPath> = root
        .members
        .iter()
        .flat_map(|member| &member.dependencies)
        .filter(|dependency| firm(dependency))
        .map(|dependency| &dependency.manifest)
        .collect();
    let mut required = BTreeSet::new();
    while let Some(manifest) = walk.pop() {
        if let Some(package) = reached.get(manifest)
            && required.insert(manifest.as_str())
        {
            walk.extend(
                package
                    .dependencies
                    .iter()
                    .filter(|dependency| dependency.kind != DependencyKind::Dev && firm(dependency))
                    .map(|dependency| &dependency.manifest),
            );
        }
    }
    required
}

fn hosted<'f>(
    root: &'f RustRoot,
    reached: &BTreeMap<&'f RepoPath, &'f UnownedPackage>,
    platforms: &Platforms,
) -> BTreeSet<&'f RepoPath> {
    let edges = |manifest: &RepoPath| -> &'f [MemberDependency] {
        match (member_of(root, manifest), reached.get(manifest)) {
            (Some(member), _) => &member.dependencies,
            (None, Some(package)) => &package.dependencies,
            (None, None) => &[],
        }
    };
    let mut live: BTreeSet<&RepoPath> = BTreeSet::new();
    let mut walk: Vec<&RepoPath> = root
        .members
        .iter()
        .flat_map(|member| &member.dependencies)
        .filter(|dependency| applies_to_target_build(dependency, platforms))
        .map(|dependency| &dependency.manifest)
        .collect();
    while let Some(manifest) = walk.pop() {
        if let Some(package) = reached.get(manifest)
            && live.insert(manifest)
        {
            walk.extend(
                package
                    .dependencies
                    .iter()
                    .filter(|dependency| {
                        dependency.kind != DependencyKind::Dev
                            && applies_to_target_build(dependency, platforms)
                    })
                    .map(|dependency| &dependency.manifest),
            );
        }
    }
    let mut pending: Vec<&RepoPath> = root
        .members
        .iter()
        .map(|member| &member.manifest)
        .chain(live.iter().copied())
        .flat_map(&edges)
        .filter(|dependency| {
            dependency.kind == DependencyKind::Build && applies_to_host_build(dependency, platforms)
        })
        .map(|dependency| &dependency.manifest)
        .collect();
    let mut hosted = BTreeSet::new();
    while let Some(manifest) = pending.pop() {
        if hosted.insert(manifest) {
            pending.extend(
                edges(manifest)
                    .iter()
                    .filter(|dependency| {
                        dependency.kind != DependencyKind::Dev
                            && applies_to_host_build(dependency, platforms)
                    })
                    .map(|dependency| &dependency.manifest),
            );
        }
    }
    hosted
}

fn applies_to_host_build(dependency: &MemberDependency, platforms: &Platforms) -> bool {
    dependency
        .platform
        .as_deref()
        .is_none_or(|predicate| platforms.holds_for_host(predicate))
}

fn applies_to_target_build(dependency: &MemberDependency, platforms: &Platforms) -> bool {
    dependency.platform.as_deref().is_none_or(|predicate| {
        if dependency.kind == DependencyKind::Build {
            platforms.holds_for_host(predicate)
        } else {
            platforms.holds_for_ordinary_edge(predicate)
        }
    })
}

fn member_of<'f>(root: &'f RustRoot, manifest: &RepoPath) -> Option<&'f RustMember> {
    root.members
        .iter()
        .find(|member| &member.manifest == manifest)
}

fn needs<'f>(
    package: &'f PackageName,
    target: &Target,
    source: &'f RepoPath,
    hosted: bool,
) -> Vec<Need<'f>> {
    let has = |kinds: &[TargetKind]| target.kinds.iter().any(|kind| kinds.contains(kind));
    let proc_macro = has(&[TargetKind::ProcMacro]);
    let unit = |kind, host| Need::Unit {
        host,
        kind,
        package,
        target: crate_name(target.name.as_str()),
        source,
    };
    let host_copy = hosted && has(&[TargetKind::Lib]);
    if has(&[TargetKind::BuildScript]) {
        let run = |host| Need::Unit {
            host,
            kind: UnitKind::Run,
            package,
            target: String::new(),
            source,
        };
        return [Some(run(false)), hosted.then(|| run(true))]
            .into_iter()
            .flatten()
            .collect();
    }
    if has(&[TargetKind::Bench]) {
        return vec![unit(UnitKind::Harness, proc_macro)];
    }
    if has(&[TargetKind::Test]) {
        return [unit(UnitKind::Harness, proc_macro)]
            .into_iter()
            .filter(|_| target.tests)
            .collect();
    }
    let mut needs = vec![unit(UnitKind::Normal, proc_macro)];
    if host_copy {
        needs.push(unit(UnitKind::Normal, true));
    }
    if target.tests {
        needs.push(unit(UnitKind::Harness, proc_macro));
    }
    needs
}

pub(crate) fn reached<'f>(
    root: &RustRoot,
    unowned: &'f BTreeMap<RepoPath, UnownedPackage>,
) -> BTreeMap<&'f RepoPath, &'f UnownedPackage> {
    let mut pending: Vec<&RepoPath> = root
        .members
        .iter()
        .flat_map(|member| &member.dependencies)
        .map(|dependency| &dependency.manifest)
        .collect();
    let mut reached = BTreeMap::new();
    while let Some(manifest) = pending.pop() {
        let Some((manifest, package)) = unowned.get_key_value(manifest) else {
            continue;
        };
        if reached.insert(manifest, package).is_none() {
            pending.extend(package.dependencies.iter().map(|d| &d.manifest));
        }
    }
    reached
}
