// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{CompileInputs, PackageName, RepoFacts, RepoPath, RustMember};

use crate::select::{claims, relative};

#[derive(Debug, Clone)]
pub struct Owner<'f> {
    name: &'f PackageName,
    manifest: &'f RepoPath,
    directory: Option<RepoPath>,
    whole: bool,
    own: bool,
}

impl Owner<'_> {
    #[must_use]
    pub fn is_own(&self) -> bool {
        self.own
    }

    #[must_use]
    pub fn name(&self) -> &PackageName {
        self.name
    }

    #[must_use]
    pub fn manifest(&self) -> &RepoPath {
        self.manifest
    }

    #[must_use]
    pub fn directory(&self) -> Option<&RepoPath> {
        self.directory.as_ref()
    }

    fn claims(&self, path: &RepoPath) -> bool {
        claims(self.directory.as_ref(), self.whole, path)
    }
}

#[derive(Debug)]
struct Unit<'f> {
    name: Option<&'f PackageName>,
    directory: Option<RepoPath>,
    sources: Vec<&'f RepoPath>,
    unowned: bool,
    owners: Vec<Owner<'f>>,
}

#[derive(Debug)]
pub struct Owners<'f> {
    units: Vec<Unit<'f>>,
    tracked: &'f BTreeSet<RepoPath>,
}

impl<'f> Owners<'f> {
    #[must_use]
    pub fn new(facts: &'f RepoFacts, tracked: &'f BTreeSet<RepoPath>) -> Self {
        let members: Vec<(&RustMember, bool)> = facts
            .rust_roots
            .iter()
            .flat_map(|root| {
                root.members
                    .iter()
                    .map(move |member| (member, member.manifest != root.manifest))
            })
            .collect();
        let mut units: Vec<Unit<'f>> = members
            .iter()
            .map(|(member, whole)| Unit {
                name: Some(&member.name),
                directory: member.directory.clone(),
                sources: member
                    .targets
                    .iter()
                    .filter_map(|t| t.source.as_ref())
                    .collect(),
                unowned: false,
                owners: vec![Owner {
                    name: &member.name,
                    manifest: &member.manifest,
                    directory: member.directory.clone(),
                    whole: *whole,
                    own: true,
                }],
            })
            .collect();
        let owned: BTreeSet<&RepoPath> = members.iter().map(|(m, _)| &m.manifest).collect();
        let mut unowned: BTreeMap<&RepoPath, Unit<'f>> = BTreeMap::new();
        for (member, _) in &members {
            let mut pending: Vec<&RepoPath> =
                member.dependencies.iter().map(|d| &d.manifest).collect();
            let mut reached = BTreeSet::new();
            while let Some(manifest) = pending.pop() {
                if owned.contains(manifest) || !reached.insert(manifest) {
                    continue;
                }
                if let Some(package) = facts.unowned.get(manifest) {
                    pending.extend(package.dependencies.iter().map(|d| &d.manifest));
                }
                let unit = unowned.entry(manifest).or_insert_with(|| Unit {
                    name: facts.unowned.get(manifest).map(|package| &package.name),
                    directory: manifest.parent(),
                    sources: Vec::new(),
                    unowned: true,
                    owners: Vec::new(),
                });
                unit.owners.push(Owner {
                    name: &member.name,
                    manifest: &member.manifest,
                    directory: unit.directory.clone(),
                    whole: true,
                    own: false,
                });
            }
        }
        units.extend(unowned.into_values());
        Self { units, tracked }
    }

    #[must_use]
    pub fn of(&self, package: Option<&str>, source: &RepoPath) -> Vec<Owner<'f>> {
        let named = package.map_or_else(Vec::new, |name| self.named(name, source));
        if !named.is_empty() {
            return named;
        }
        let targets: Vec<Owner<'f>> = self
            .units
            .iter()
            .filter(|unit| unit.sources.contains(&source))
            .flat_map(|unit| unit.owners.iter().cloned())
            .collect();
        if !targets.is_empty() {
            return targets;
        }
        let Some(package) = self.package(source).map(Package::directory) else {
            return self
                .units
                .iter()
                .filter(|unit| relative(unit.directory.as_ref(), source).is_some())
                .max_by_key(|unit| unit.directory.as_ref().map_or(0, |d| d.as_str().len()))
                .map(|unit| unit.owners.clone())
                .unwrap_or_default();
        };
        match self.units.iter().find(|unit| unit.directory == package) {
            Some(unit) => unit.owners.clone(),
            None => self
                .units
                .iter()
                .filter(|unit| unit.unowned)
                .flat_map(|unit| &unit.owners)
                .map(|owner| Owner {
                    directory: package.clone(),
                    whole: true,
                    ..owner.clone()
                })
                .collect(),
        }
    }

    fn named(&self, name: &str, source: &RepoPath) -> Vec<Owner<'f>> {
        let foreign = match self.package(source) {
            Some(Package::Folder(folder)) => !self
                .units
                .iter()
                .any(|unit| unit.directory.as_ref() == Some(&folder)),
            Some(Package::Root) | None => false,
        };
        let named: Vec<&Unit<'f>> = self
            .units
            .iter()
            .filter(|unit| unit.name.is_some_and(|own| own.as_str() == name))
            .collect();
        if foreign || !named.iter().any(|unit| unit.unowned) {
            return Vec::new();
        }
        named
            .into_iter()
            .flat_map(|unit| unit.owners.iter().cloned())
            .collect()
    }

    fn package(&self, source: &RepoPath) -> Option<Package> {
        let mut directory = source.parent();
        while let Some(folder) = directory {
            if RepoPath::new(format!("{folder}/Cargo.toml"))
                .is_ok_and(|manifest| self.tracked.contains(&manifest))
            {
                return Some(Package::Folder(folder));
            }
            directory = folder.parent();
        }
        self.tracked
            .contains(&RepoPath::from_static("Cargo.toml"))
            .then_some(Package::Root)
    }
}

enum Package {
    Root,
    Folder(RepoPath),
}

impl Package {
    fn directory(self) -> Option<RepoPath> {
        match self {
            Self::Root => None,
            Self::Folder(folder) => Some(folder),
        }
    }
}

pub fn add_reads(
    found: &mut CompileInputs,
    tracked: &BTreeSet<RepoPath>,
    owner: &Owner<'_>,
    reads: &[Option<RepoPath>],
    declared: bool,
    targets: &[RepoPath],
) {
    for read in reads {
        let Some(read) = read else {
            if !tracked.iter().all(|path| owner.claims(path)) {
                found.entry(None).or_default().insert(owner.name().clone());
            }
            continue;
        };
        let generated = targets
            .iter()
            .any(|target| read == target || read.as_str().starts_with(&format!("{target}/")));
        let mut below = tracked
            .range(read.clone()..)
            .take_while(|path| path.as_str().starts_with(read.as_str()))
            .filter(|path| {
                path.as_str().len() == read.as_str().len()
                    || path.as_str()[read.as_str().len()..].starts_with('/')
            })
            .peekable();
        let outside = match below.peek() {
            Some(_) => !below.all(|path| owner.claims(path)),
            None => declared && !generated && !owner.claims(read),
        };
        if outside {
            found
                .entry(Some(read.clone()))
                .or_default()
                .insert(owner.name().clone());
        }
    }
}
