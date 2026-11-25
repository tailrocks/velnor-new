// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    DependencyKind, MemberDependency, RepoPath, RustMember, TargetKind, UnownedPackage,
};

use super::platform::Platform;

pub(super) struct Request<'a> {
    pub(super) all: bool,
    pub(super) defaults: bool,
    pub(super) names: &'a [&'a str],
}

#[derive(Clone, Copy)]
struct Package<'a> {
    name: &'a str,
    features: &'a BTreeMap<String, Vec<String>>,
    dependencies: &'a [MemberDependency],
    scripted: bool,
}

#[derive(Default)]
struct State {
    visited: bool,
    root: bool,
    tested: bool,
    features: BTreeSet<String>,
    dependencies: BTreeSet<String>,
    weak: BTreeSet<(String, String)>,
}

pub(super) struct Resolver<'a, 'p> {
    members: &'a [RustMember],
    unowned: &'a BTreeMap<RepoPath, UnownedPackage>,
    platform: &'p dyn Fn(bool) -> Option<&'p Platform>,
    states: BTreeMap<&'a RepoPath, State>,
}

fn script_of(member: &RustMember) -> bool {
    member
        .targets
        .iter()
        .any(|target| target.kinds.contains(&TargetKind::BuildScript))
}

pub(super) fn tests_of(member: &RustMember, doctests: bool) -> bool {
    member.targets.iter().any(|target| {
        target.tests
            || (doctests
                && target.doctest
                && target
                    .kinds
                    .iter()
                    .any(|kind| matches!(kind, TargetKind::Lib | TargetKind::ProcMacro)))
            || target.kinds.iter().any(|kind| {
                matches!(
                    kind,
                    TargetKind::Test | TargetKind::Bench | TargetKind::Example
                )
            })
    })
}

fn implicit(package: &Package<'_>, alias: &str) -> bool {
    let suppressed = format!("dep:{alias}");
    package
        .dependencies
        .iter()
        .any(|dependency| dependency.optional && dependency.feature == alias)
        && !package
            .features
            .values()
            .flatten()
            .any(|entry| *entry == suppressed)
}

impl<'a, 'p> Resolver<'a, 'p> {
    pub(super) fn new(
        members: &'a [RustMember],
        unowned: &'a BTreeMap<RepoPath, UnownedPackage>,
        platform: &'p dyn Fn(bool) -> Option<&'p Platform>,
    ) -> Self {
        Self {
            members,
            unowned,
            platform,
            states: BTreeMap::new(),
        }
    }

    pub(super) fn mark(&mut self, member: &'a RustMember, tested: bool) {
        let state = self.states.entry(&member.manifest).or_default();
        state.root = true;
        state.tested = tested;
    }

    pub(super) fn start(&mut self, member: &'a RustMember, request: &Request<'_>) {
        self.visit(&member.manifest);
        let Some(package) = self.package(&member.manifest) else {
            return;
        };
        if request.all {
            for name in package.features.keys() {
                self.activate(&member.manifest, name);
            }
            let optional: Vec<&str> = package
                .dependencies
                .iter()
                .filter(|dependency| dependency.optional)
                .map(|dependency| dependency.feature.as_str())
                .collect();
            for alias in optional {
                if implicit(&package, alias) {
                    self.activate(&member.manifest, alias);
                } else {
                    self.enable(&member.manifest, alias);
                }
            }
            return;
        }
        if request.defaults {
            self.default_features(&member.manifest);
        }
        for name in request.names {
            match name.split_once('/') {
                Some((owner, feature)) if owner == package.name => {
                    self.activate(&member.manifest, feature);
                }
                Some(_) => self.apply(&member.manifest, name),
                None if name.starts_with("dep:") => self.apply(&member.manifest, name),
                None if self.declares(&member.manifest, name) => {
                    self.activate(&member.manifest, name);
                }
                None => {}
            }
        }
    }

    pub(super) fn enabled(&self, manifest: &RepoPath) -> BTreeSet<String> {
        self.states
            .get(manifest)
            .map_or_else(BTreeSet::new, |state| state.features.clone())
    }

    pub(super) fn packages(&self) -> BTreeSet<&'a str> {
        self.states
            .iter()
            .filter(|(_, state)| state.visited && !state.root)
            .filter_map(|(manifest, _)| self.package(manifest))
            .map(|package| package.name)
            .collect()
    }

    fn package(&self, manifest: &RepoPath) -> Option<Package<'a>> {
        if let Some(package) = self.unowned.get(manifest) {
            return Some(Package {
                name: package.name.as_str(),
                features: &package.features,
                dependencies: &package.dependencies,
                scripted: true,
            });
        }
        self.members
            .iter()
            .find(|member| member.manifest == *manifest)
            .map(|member| Package {
                name: member.name.as_str(),
                features: &member.features,
                dependencies: &member.dependencies,
                scripted: script_of(member),
            })
    }

    fn default_features(&mut self, manifest: &'a RepoPath) {
        if self
            .package(manifest)
            .is_some_and(|package| package.features.contains_key("default"))
        {
            self.activate(manifest, "default");
        }
    }

    fn declares(&self, manifest: &RepoPath, name: &str) -> bool {
        self.package(manifest)
            .is_some_and(|package| package.features.contains_key(name) || implicit(&package, name))
    }

    fn live(
        &self,
        manifest: &RepoPath,
        package: Package<'a>,
        dependency: &MemberDependency,
    ) -> bool {
        let Some(state) = self.states.get(manifest) else {
            return false;
        };
        let kind = match dependency.kind {
            DependencyKind::Dev => state.root && state.tested,
            DependencyKind::Build => package.scripted,
            DependencyKind::Normal => true,
        };
        kind && dependency.platform.as_deref().is_none_or(|predicate| {
            (self.platform)(dependency.kind == DependencyKind::Build)
                .and_then(|platform| platform.active(predicate))
                == Some(true)
        })
    }

    fn visit(&mut self, manifest: &'a RepoPath) {
        let state = self.states.entry(manifest).or_default();
        if state.visited {
            return;
        }
        state.visited = true;
        let Some(package) = self.package(manifest) else {
            return;
        };
        for dependency in package.dependencies {
            if !dependency.optional && self.live(manifest, package, dependency) {
                self.enter(dependency);
            }
        }
    }

    fn enter(&mut self, dependency: &'a MemberDependency) {
        self.visit(&dependency.manifest);
        if dependency.default_features {
            self.default_features(&dependency.manifest);
        }
        for feature in &dependency.features {
            self.activate(&dependency.manifest, feature);
        }
    }

    fn enable(&mut self, manifest: &'a RepoPath, alias: &str) {
        let Some(state) = self.states.get_mut(manifest) else {
            return;
        };
        if !state.dependencies.insert(alias.to_owned()) {
            return;
        }
        let waiting: Vec<String> = state
            .weak
            .iter()
            .filter(|(name, _)| name == alias)
            .map(|(_, feature)| feature.clone())
            .collect();
        let Some(package) = self.package(manifest) else {
            return;
        };
        for dependency in package.dependencies {
            if dependency.feature == alias && self.live(manifest, package, dependency) {
                self.enter(dependency);
            }
        }
        for feature in waiting {
            self.forward(manifest, alias, &feature);
        }
    }

    fn present(&self, manifest: &RepoPath, alias: &str) -> bool {
        let Some(package) = self.package(manifest) else {
            return false;
        };
        let state = self.states.get(manifest);
        package.dependencies.iter().any(|dependency| {
            dependency.feature == alias
                && (!dependency.optional
                    || state.is_some_and(|state| state.dependencies.contains(alias)))
                && self.live(manifest, package, dependency)
        })
    }

    fn forward(&mut self, manifest: &'a RepoPath, alias: &str, feature: &str) {
        let Some(package) = self.package(manifest) else {
            return;
        };
        for dependency in package.dependencies {
            if dependency.feature == alias
                && (!dependency.optional
                    || self
                        .states
                        .get(manifest)
                        .is_some_and(|state| state.dependencies.contains(alias)))
                && self.live(manifest, package, dependency)
            {
                self.visit(&dependency.manifest);
                self.apply(&dependency.manifest, feature);
            }
        }
    }

    fn apply(&mut self, manifest: &'a RepoPath, entry: &str) {
        if let Some(alias) = entry.strip_prefix("dep:") {
            self.enable(manifest, alias);
        } else if let Some((left, feature)) = entry.split_once('/') {
            if let Some(alias) = left.strip_suffix('?') {
                if self.present(manifest, alias) {
                    self.forward(manifest, alias, feature);
                } else if let Some(state) = self.states.get_mut(manifest) {
                    state.weak.insert((alias.to_owned(), feature.to_owned()));
                }
            } else {
                self.enable(manifest, left);
                self.forward(manifest, left, feature);
            }
        } else {
            self.activate(manifest, entry);
        }
    }

    fn activate(&mut self, manifest: &'a RepoPath, feature: &str) {
        let Some(state) = self.states.get_mut(manifest) else {
            return;
        };
        if !state.features.insert(feature.to_owned()) {
            return;
        }
        let Some(package) = self.package(manifest) else {
            return;
        };
        match package.features.get(feature) {
            Some(entries) => {
                for entry in entries {
                    self.apply(manifest, entry);
                }
            }
            None => {
                if implicit(&package, feature) {
                    self.enable(manifest, feature);
                }
            }
        }
    }
}
