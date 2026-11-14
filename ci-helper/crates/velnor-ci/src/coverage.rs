// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{PackageName, RepoPath, RustRoot, Target, TargetKind, UnownedPackage};

pub const HOST: &str = "debug";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnitKind {
    Normal,
    Harness,
    Run,
}

impl UnitKind {
    #[must_use]
    pub fn of_fingerprint(name: &str) -> Option<Self> {
        if std::path::Path::new(name).extension().is_some() {
            None
        } else if name.starts_with("test-") {
            Some(Self::Harness)
        } else if ["lib-", "bin-", "example-"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            Some(Self::Normal)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Unit {
    pub configuration: String,
    pub kind: UnitKind,
    pub source: RepoPath,
}

#[derive(Debug, Default)]
pub struct Built {
    pub units: BTreeSet<Unit>,
    pub libraries: BTreeSet<(String, PackageName)>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Gap {
    pub configuration: String,
    pub missing: Vec<String>,
}

enum Need<'f> {
    Unit {
        host: bool,
        kind: UnitKind,
        source: &'f RepoPath,
    },
    Package {
        directory: Option<RepoPath>,
        name: &'f PackageName,
    },
}

#[must_use]
pub fn gap(
    root: &RustRoot,
    unowned: &BTreeMap<RepoPath, UnownedPackage>,
    built: &Built,
) -> Option<Gap> {
    let mut groups: Vec<(String, Vec<Need<'_>>)> = root
        .members
        .iter()
        .flat_map(|member| {
            member
                .targets
                .iter()
                .filter(|target| !target.requires_features)
                .filter_map(move |target| {
                    let source = target.source.as_ref()?;
                    let label = format!(
                        "{} ({}) target {} ({source}",
                        member.name, member.manifest, target.name
                    );
                    Some((label, needs(target, source)))
                })
        })
        .collect();
    groups.extend(
        reached(root, unowned)
            .into_iter()
            .map(|(manifest, package)| {
                (
                    format!("path dependency {} ({manifest}", package.name),
                    vec![Need::Package {
                        directory: manifest.parent(),
                        name: &package.name,
                    }],
                )
            }),
    );
    let packages: Vec<Option<RepoPath>> = root
        .members
        .iter()
        .map(|member| member.directory.clone())
        .chain(unowned.keys().map(RepoPath::parent))
        .collect();
    let mut candidates: Vec<&str> = vec![HOST];
    candidates.extend(
        built
            .units
            .iter()
            .map(|unit| unit.configuration.as_str())
            .filter(|configuration| configuration.ends_with(&format!("/{HOST}"))),
    );
    candidates.dedup();
    candidates
        .into_iter()
        .map(|configuration| Gap {
            configuration: configuration.to_owned(),
            missing: groups
                .iter()
                .filter_map(|(label, needs)| {
                    let wanted: Vec<&Need<'_>> = needs
                        .iter()
                        .filter(|need| wanted(need, configuration, built))
                        .collect();
                    let found = wanted
                        .iter()
                        .filter(|need| found(need, configuration, built, &packages))
                        .count();
                    (found < wanted.len())
                        .then(|| format!("{label}, {found} of {} units)", wanted.len()))
                })
                .collect(),
        })
        .min_by_key(|gap| gap.missing.len())
        .filter(|gap| !gap.missing.is_empty())
}

fn needs<'f>(target: &Target, source: &'f RepoPath) -> Vec<Need<'f>> {
    let has = |kinds: &[TargetKind]| target.kinds.iter().any(|kind| kinds.contains(kind));
    let host = has(&[TargetKind::ProcMacro]);
    let unit = |kind| Need::Unit { host, kind, source };
    if has(&[TargetKind::BuildScript]) {
        return vec![Need::Unit {
            host: false,
            kind: UnitKind::Run,
            source,
        }];
    }
    if has(&[TargetKind::Bench]) {
        return vec![unit(UnitKind::Harness)];
    }
    if has(&[TargetKind::Test]) {
        return [unit(UnitKind::Harness)]
            .into_iter()
            .filter(|_| target.tests)
            .collect();
    }
    let mut needs = vec![unit(UnitKind::Normal)];
    if target.tests {
        needs.push(unit(UnitKind::Harness));
    }
    needs
}

fn wanted(need: &Need<'_>, configuration: &str, built: &Built) -> bool {
    match need {
        Need::Unit { .. } => true,
        Need::Package { name, .. } => [configuration, HOST].iter().any(|place| {
            built
                .libraries
                .contains(&((*place).to_owned(), (*name).clone()))
        }),
    }
}

fn found(
    need: &Need<'_>,
    configuration: &str,
    built: &Built,
    packages: &[Option<RepoPath>],
) -> bool {
    match need {
        Need::Unit { host, kind, source } => built.units.contains(&Unit {
            configuration: if *host { HOST } else { configuration }.to_owned(),
            kind: *kind,
            source: (*source).clone(),
        }),
        Need::Package { directory, .. } => built.units.iter().any(|unit| {
            unit.kind == UnitKind::Normal
                && [configuration, HOST].contains(&unit.configuration.as_str())
                && package_of(&unit.source, packages) == Some(directory)
        }),
    }
}

fn package_of<'p>(
    source: &RepoPath,
    packages: &'p [Option<RepoPath>],
) -> Option<&'p Option<RepoPath>> {
    packages
        .iter()
        .filter(|directory| {
            directory
                .as_ref()
                .is_none_or(|directory| source.as_str().starts_with(&format!("{directory}/")))
        })
        .max_by_key(|directory| directory.as_ref().map_or(0, |d| d.as_str().len()))
}

fn reached<'f>(
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
