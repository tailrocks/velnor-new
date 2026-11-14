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
    pub scripts: BTreeSet<RepoPath>,
    pub builders: BTreeSet<(String, PackageName)>,
    pub orphans: BTreeSet<(String, String, Fingerprint)>,
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
        package: &'f PackageName,
        target: String,
        source: &'f RepoPath,
    },
    Library(&'f [Option<RepoPath>], &'f PackageName),
    Run(&'f [Option<RepoPath>], &'f PackageName),
}

type Paths<'f> = BTreeMap<&'f PackageName, (Vec<&'f RepoPath>, Vec<Option<RepoPath>>)>;

struct Group<'f> {
    label: String,
    needs: Vec<Need<'f>>,
}

#[must_use]
pub fn gaps(
    root: &RustRoot,
    unowned: &BTreeMap<RepoPath, UnownedPackage>,
    built: &Built,
    triples: &BTreeSet<String>,
) -> Vec<Gap> {
    let reached = reached(root, unowned);
    let mut paths: Paths<'_> = BTreeMap::new();
    for (manifest, package) in &reached {
        let (manifests, directories) = paths.entry(&package.name).or_default();
        manifests.push(manifest);
        directories.push(manifest.parent());
    }
    let groups = groups(root, &paths);
    let packages: Vec<Option<RepoPath>> = root
        .members
        .iter()
        .map(|member| member.directory.clone())
        .chain(unowned.keys().map(RepoPath::parent))
        .collect();
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
            .filter_map(|group| missing(group, configuration, built, &packages))
            .chain(orphans(configuration, built, &owned))
            .collect(),
    };
    let active = active(root, built, triples);
    let mut found: Vec<Gap> = if active.is_empty() {
        let mut candidates = vec![HOST];
        candidates.extend(
            built
                .units
                .iter()
                .map(|unit| unit.configuration.as_str())
                .filter(|configuration| profiled(configuration, triples) && *configuration != HOST),
        );
        candidates.dedup();
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

fn groups<'f>(root: &'f RustRoot, paths: &'f Paths<'f>) -> Vec<Group<'f>> {
    let mut groups: Vec<Group<'f>> = root
        .members
        .iter()
        .flat_map(|member| {
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
                        needs: needs(&member.name, target, source),
                    })
                })
        })
        .collect();
    groups.extend(paths.iter().map(|(name, (manifests, directories))| {
        let manifests: Vec<String> = manifests.iter().map(ToString::to_string).collect();
        Group {
            label: format!("path dependency {name} ({}", manifests.join(" or ")),
            needs: vec![
                Need::Library(directories, name),
                Need::Run(directories, name),
            ],
        }
    }));
    groups
}

fn active<'b>(root: &RustRoot, built: &'b Built, triples: &BTreeSet<String>) -> Vec<&'b str> {
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
                && profiled(&unit.configuration, triples)
                && tested.contains(&(unit.package.as_str(), unit.target.clone()))
        })
        .map(|unit| unit.configuration.as_str())
        .collect();
    active.dedup();
    active
}

fn profiled(configuration: &str, triples: &BTreeSet<String>) -> bool {
    configuration == HOST
        || configuration
            .strip_suffix(&format!("/{HOST}"))
            .is_some_and(|triple| triples.contains(triple))
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
                "{package} target {} ({} unit with a cargo fingerprint and no dep-info)",
                fingerprint.target,
                match fingerprint.kind {
                    UnitKind::Normal => "normal",
                    UnitKind::Harness => "test harness",
                    UnitKind::Run => "build script run",
                }
            )
        })
}

fn missing(
    group: &Group<'_>,
    configuration: &str,
    built: &Built,
    packages: &[Option<RepoPath>],
) -> Option<String> {
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
            Need::Library(directories, name) => places
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
                            && within(&unit.source, directories, packages)
                    })
                })
                .collect(),
            Need::Run(directories, name) => {
                let scripts: Vec<bool> = built
                    .scripts
                    .iter()
                    .filter(|script| within(script, directories, packages))
                    .map(|script| {
                        built.units.iter().any(|unit| {
                            unit.kind == UnitKind::Run
                                && places.contains(&unit.configuration.as_str())
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

fn needs<'f>(package: &'f PackageName, target: &Target, source: &'f RepoPath) -> Vec<Need<'f>> {
    let has = |kinds: &[TargetKind]| target.kinds.iter().any(|kind| kinds.contains(kind));
    let host = has(&[TargetKind::ProcMacro]);
    let unit = |kind| Need::Unit {
        host,
        kind,
        package,
        target: crate_name(target.name.as_str()),
        source,
    };
    if has(&[TargetKind::BuildScript]) {
        return vec![Need::Unit {
            host: false,
            kind: UnitKind::Run,
            package,
            target: String::new(),
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

fn within(
    source: &RepoPath,
    directories: &[Option<RepoPath>],
    packages: &[Option<RepoPath>],
) -> bool {
    packages
        .iter()
        .filter(|directory| {
            directory
                .as_ref()
                .is_none_or(|directory| source.as_str().starts_with(&format!("{directory}/")))
        })
        .max_by_key(|directory| directory.as_ref().map_or(0, |d| d.as_str().len()))
        .is_some_and(|package| directories.contains(package))
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
