// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use velnor_model::{CompileInputs, PackageName, RepoFacts, RepoPath, RustMember};

use crate::error::{DetectError, Reason};
use crate::select::{claims, relative};

pub const INPUTS_LOCK: RepoPath = RepoPath::from_static(".velnor/rust-inputs.lock");
const REPOSITORY: &str = ".";

#[must_use]
pub fn dep_info_paths(text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(prerequisites)
        .flat_map(words)
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

#[must_use]
pub fn rerun_if_changed(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            line.strip_prefix("cargo::rerun-if-changed=")
                .or_else(|| line.strip_prefix("cargo:rerun-if-changed="))
        })
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect()
}

fn prerequisites(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    (0..bytes.len())
        .find(|&at| bytes[at] == b':' && bytes.get(at + 1).is_none_or(|next| *next == b' '))
        .map(|at| &line[at + 1..])
}

fn words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(' ') => word.push(' '),
                next => {
                    word.push('\\');
                    word.extend(next);
                }
            },
            ' ' if !word.is_empty() => words.push(std::mem::take(&mut word)),
            ' ' => {}
            other => word.push(other),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Located {
    Inside(RepoPath),
    Repository,
    Outside,
}

#[must_use]
pub fn locate(repository: &Path, base: &Path, raw: &str) -> Located {
    let full = normalize(&base.join(raw));
    match full.strip_prefix(repository) {
        Ok(inside) if inside.as_os_str().is_empty() => Located::Repository,
        Ok(inside) => inside
            .to_str()
            .and_then(|text| RepoPath::new(text).ok())
            .map_or(Located::Outside, Located::Inside),
        Err(_) => Located::Outside,
    }
}

#[must_use]
pub fn normalize(path: &Path) -> PathBuf {
    let mut full = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                full.pop();
            }
            Component::CurDir => {}
            other => full.push(other),
        }
    }
    full
}

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
    pub fn of(&self, source: &RepoPath) -> Vec<Owner<'f>> {
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

#[must_use]
pub fn unlisted(found: &CompileInputs, listed: &CompileInputs) -> CompileInputs {
    found
        .iter()
        .filter_map(|(path, packages)| {
            let missing: BTreeSet<PackageName> = packages
                .iter()
                .filter(|package| !listed.get(path).is_some_and(|l| l.contains(*package)))
                .cloned()
                .collect();
            (!missing.is_empty()).then(|| (path.clone(), missing))
        })
        .collect()
}

#[must_use]
pub fn format_inputs_lock(inputs: &CompileInputs) -> String {
    inputs
        .iter()
        .flat_map(|(path, packages)| {
            packages.iter().map(move |package| match path {
                Some(path) => format!("{path}\t{package}\n"),
                None => format!("{REPOSITORY}\t{package}\n"),
            })
        })
        .collect()
}

pub fn parse_inputs_lock(bytes: &[u8]) -> Result<CompileInputs, DetectError> {
    let invalid = |reason| DetectError::invalid(&INPUTS_LOCK, reason);
    let text = std::str::from_utf8(bytes).map_err(|_| invalid(Reason::NotUtf8))?;
    let mut inputs: CompileInputs = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let entry = line.split_once('\t').and_then(|(path, package)| {
            let path = match path {
                REPOSITORY => None,
                path => Some(RepoPath::new(path).ok()?),
            };
            Some((path, PackageName::new(package).ok()?))
        });
        let (path, package) = entry.ok_or_else(|| invalid(Reason::LockLine(index + 1)))?;
        inputs.entry(path).or_default().insert(package);
    }
    Ok(inputs)
}
