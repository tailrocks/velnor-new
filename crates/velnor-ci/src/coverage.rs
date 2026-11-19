// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{PackageName, RepoPath, RustRoot, TargetKind, UnownedPackage};

use crate::needs::{Group, Need, Paths, groups, reached};
use crate::platform::Platforms;

pub const HOST: &str = "debug";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnitKind {
    Normal,
    Harness,
    Run,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fingerprint {
    pub kind: UnitKind,
    pub target: String,
}

impl Fingerprint {
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        if std::path::Path::new(name).extension().is_some() {
            return None;
        }
        let (kind, rest, prefixes) = match name.strip_prefix("test-") {
            Some(rest) => (
                UnitKind::Harness,
                rest,
                &["lib-", "bin-", "example-", "integration-test-", "bench-"][..],
            ),
            None => (UnitKind::Normal, name, &["lib-", "bin-", "example-"][..]),
        };
        let target = prefixes
            .iter()
            .find_map(|prefix| rest.strip_prefix(prefix))?;
        Some(Self {
            kind,
            target: crate_name(target),
        })
    }
}

#[must_use]
pub fn crate_name(target: &str) -> String {
    target.replace('-', "_")
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Unit {
    pub configuration: String,
    pub kind: UnitKind,
    pub package: String,
    pub target: String,
    pub source: RepoPath,
}

#[derive(Debug, Default)]
pub struct Built {
    pub units: BTreeSet<Unit>,
    pub libraries: BTreeSet<(String, PackageName)>,
    pub scripts: BTreeSet<(String, RepoPath)>,
    pub builders: BTreeSet<(String, PackageName)>,
    pub orphans: BTreeSet<(String, String, Fingerprint)>,
    pub placed: BTreeSet<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Gap {
    pub configuration: String,
    pub missing: Vec<String>,
}

#[must_use]
pub fn gaps(
    root: &RustRoot,
    unowned: &BTreeMap<RepoPath, UnownedPackage>,
    built: &Built,
    platforms: &Platforms,
) -> Vec<Gap> {
    let reached = reached(root, unowned);
    let mut paths: Paths<'_> = BTreeMap::new();
    for (manifest, package) in &reached {
        paths.entry(&package.name).or_default().push(manifest);
    }
    let groups = groups(root, &paths, &reached, platforms);
    let owned: BTreeSet<&str> = root
        .members
        .iter()
        .map(|member| member.name.as_str())
        .chain(reached.values().map(|package| package.name.as_str()))
        .collect();
    let check = |configuration: &str| Gap {
        configuration: configuration.to_owned(),
        missing: groups
            .iter()
            .filter_map(|group| missing(group, configuration, built))
            .chain(orphans(configuration, built, &owned))
            .collect(),
    };
    let active = active(root, built);
    let mut found: Vec<Gap> = if active.is_empty() {
        let others: BTreeSet<&str> = built
            .units
            .iter()
            .map(|unit| unit.configuration.as_str())
            .chain(
                built
                    .orphans
                    .iter()
                    .filter(|(_, package, _)| owned.contains(package.as_str()))
                    .map(|(at, _, _)| at.as_str()),
            )
            .filter(|configuration| profiled(configuration, built) && *configuration != HOST)
            .collect();
        let mut candidates = vec![HOST];
        candidates.extend(others);
        candidates
            .into_iter()
            .map(check)
            .min_by_key(|gap| gap.missing.len())
            .into_iter()
            .collect()
    } else {
        active
            .iter()
            .map(|configuration| check(configuration))
            .collect()
    };
    if !found.iter().any(|gap| gap.configuration == HOST) {
        found.push(Gap {
            configuration: HOST.to_owned(),
            missing: orphans(HOST, built, &owned).collect(),
        });
    }
    found.retain(|gap| !gap.missing.is_empty());
    found
}

fn active<'b>(root: &RustRoot, built: &'b Built) -> Vec<&'b str> {
    let tested: BTreeSet<(&str, String)> = root
        .members
        .iter()
        .flat_map(|member| {
            member
                .targets
                .iter()
                .filter(|target| !target.kinds.contains(&TargetKind::ProcMacro))
                .map(|target| (member.name.as_str(), crate_name(target.name.as_str())))
        })
        .collect();
    let mut active: Vec<&str> = built
        .units
        .iter()
        .filter(|unit| {
            unit.kind == UnitKind::Harness
                && profiled(&unit.configuration, built)
                && tested.contains(&(unit.package.as_str(), unit.target.clone()))
        })
        .map(|unit| unit.configuration.as_str())
        .chain(
            built
                .orphans
                .iter()
                .filter(|(at, package, fingerprint)| {
                    fingerprint.kind == UnitKind::Harness
                        && profiled(at, built)
                        && tested.contains(&(package.as_str(), fingerprint.target.clone()))
                })
                .map(|(at, _, _)| at.as_str()),
        )
        .collect();
    active.sort_unstable();
    active.dedup();
    active
}

fn profiled(configuration: &str, built: &Built) -> bool {
    configuration == HOST
        || configuration
            .strip_suffix(&format!("/{HOST}"))
            .is_some_and(|directory| built.placed.contains(directory))
}

fn orphans<'b>(
    configuration: &'b str,
    built: &'b Built,
    owned: &'b BTreeSet<&str>,
) -> impl Iterator<Item = String> + 'b {
    built
        .orphans
        .iter()
        .filter(move |(at, package, _)| at == configuration && owned.contains(package.as_str()))
        .map(|(_, package, fingerprint)| {
            format!(
                "{package} target {} ({})",
                fingerprint.target,
                match fingerprint.kind {
                    UnitKind::Normal => "normal unit with a cargo fingerprint and no dep-info",
                    UnitKind::Harness =>
                        "test harness unit with a cargo fingerprint and no dep-info",
                    UnitKind::Run => "build script run with a cargo fingerprint and no output",
                }
            )
        })
}

fn missing(group: &Group<'_>, configuration: &str, built: &Built) -> Option<String> {
    let places: Vec<&str> = if configuration == HOST {
        vec![HOST]
    } else {
        vec![configuration, HOST]
    };
    let mut wanted = 0;
    let mut found = 0;
    for need in &group.needs {
        let units: Vec<bool> = match need {
            Need::Unit {
                host,
                kind,
                package,
                target,
                source,
            } => vec![built.units.contains(&Unit {
                configuration: if *host { HOST } else { configuration }.to_owned(),
                kind: *kind,
                package: package.to_string(),
                target: target.clone(),
                source: (*source).clone(),
            })],
            Need::Library { name, required } => {
                let libraries: Vec<bool> = places
                    .iter()
                    .filter(|place| {
                        built
                            .libraries
                            .contains(&((**place).to_owned(), (*name).clone()))
                    })
                    .map(|place| {
                        built.units.iter().any(|unit| {
                            unit.kind == UnitKind::Normal
                                && unit.configuration == *place
                                && unit.package == name.as_str()
                        })
                    })
                    .collect();
                if libraries.is_empty() && *required {
                    vec![false]
                } else {
                    libraries
                }
            }
            Need::Run(name) => {
                let scripts: Vec<bool> = built
                    .scripts
                    .iter()
                    .filter(|(package, _)| package == name.as_str())
                    .map(|(package, script)| {
                        built.units.iter().any(|unit| {
                            unit.kind == UnitKind::Run
                                && places.contains(&unit.configuration.as_str())
                                && unit.package == *package
                                && unit.source == *script
                        })
                    })
                    .collect();
                let compiled = built.builders.contains(&(HOST.to_owned(), (*name).clone()));
                if scripts.is_empty() && compiled {
                    vec![false]
                } else {
                    scripts
                }
            }
        };
        wanted += units.len();
        found += units.iter().filter(|present| **present).count();
    }
    (found < wanted).then(|| format!("{}, {found} of {wanted} units)", group.label))
}
