// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use velnor_model::{CompileInputs, PackageName, RepoFacts, RepoPath, RustMember, TargetKind};

use crate::error::{DetectError, Reason};
use crate::select::{claims, relative};

pub const INPUTS_LOCK: RepoPath = RepoPath::from_static(".velnor/rust-inputs.lock");

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
    let mut full = PathBuf::new();
    for component in base.join(raw).components() {
        match component {
            Component::ParentDir => {
                full.pop();
            }
            Component::CurDir => {}
            other => full.push(other),
        }
    }
    match full.strip_prefix(repository) {
        Ok(inside) if inside.as_os_str().is_empty() => Located::Repository,
        Ok(inside) => inside
            .to_str()
            .and_then(|text| RepoPath::new(text).ok())
            .map_or(Located::Outside, Located::Inside),
        Err(_) => Located::Outside,
    }
}

#[derive(Debug, Clone)]
pub struct Owner<'f> {
    name: &'f PackageName,
    manifest: &'f RepoPath,
    directory: Option<RepoPath>,
    whole: bool,
}

impl Owner<'_> {
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

struct Unit<'f> {
    name: &'f PackageName,
    directory: Option<RepoPath>,
    build_script: bool,
    unowned: bool,
    owners: Vec<Owner<'f>>,
}

fn units(facts: &RepoFacts) -> Vec<Unit<'_>> {
    let members: Vec<(&RustMember, bool)> = facts
        .rust_roots
        .iter()
        .flat_map(|root| {
            root.members
                .iter()
                .map(move |member| (member, member.manifest != root.manifest))
        })
        .collect();
    let mut units: Vec<Unit<'_>> = members
        .iter()
        .map(|(member, whole)| Unit {
            name: &member.name,
            directory: member.directory.clone(),
            build_script: member
                .targets
                .iter()
                .any(|target| target.kinds.contains(&TargetKind::BuildScript)),
            unowned: false,
            owners: vec![Owner {
                name: &member.name,
                manifest: &member.manifest,
                directory: member.directory.clone(),
                whole: *whole,
            }],
        })
        .collect();
    let mut unowned: BTreeMap<&RepoPath, Unit<'_>> = BTreeMap::new();
    for (member, _) in &members {
        for dependency in &member.dependencies {
            if members
                .iter()
                .any(|(m, _)| m.manifest == dependency.manifest)
            {
                continue;
            }
            let unit = unowned.entry(&dependency.manifest).or_insert_with(|| Unit {
                name: &dependency.name,
                directory: dependency.manifest.parent(),
                build_script: true,
                unowned: true,
                owners: Vec::new(),
            });
            unit.owners.push(Owner {
                name: &member.name,
                manifest: &member.manifest,
                directory: unit.directory.clone(),
                whole: true,
            });
        }
    }
    units.extend(unowned.into_values());
    units
}

#[must_use]
pub fn source_owners<'f>(facts: &'f RepoFacts, source: &RepoPath) -> Vec<Owner<'f>> {
    let units = units(facts);
    match units
        .iter()
        .filter(|unit| relative(unit.directory.as_ref(), source).is_some())
        .max_by_key(|unit| unit.directory.as_ref().map_or(0, |d| d.as_str().len()))
    {
        Some(unit) => unit.owners.clone(),
        None => units
            .iter()
            .filter(|unit| unit.unowned)
            .flat_map(|unit| unit.owners.iter().cloned())
            .collect(),
    }
}

#[must_use]
pub fn build_script_owners<'f>(facts: &'f RepoFacts, package: &str) -> Vec<Owner<'f>> {
    units(facts)
        .into_iter()
        .filter(|unit| unit.build_script && unit.name.as_str() == package)
        .flat_map(|unit| unit.owners)
        .collect()
}

pub fn add_reads(
    found: &mut CompileInputs,
    tracked: &BTreeSet<RepoPath>,
    owner: &Owner<'_>,
    reads: &[RepoPath],
    declared: bool,
) {
    for read in reads {
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
            None => declared && !owner.claims(read),
        };
        if outside {
            found
                .entry(read.clone())
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
            packages
                .iter()
                .map(move |package| format!("{path}\t{package}\n"))
        })
        .collect()
}

pub fn parse_inputs_lock(bytes: &[u8]) -> Result<CompileInputs, DetectError> {
    let invalid = |reason| DetectError::invalid(&INPUTS_LOCK, reason);
    let text = std::str::from_utf8(bytes).map_err(|_| invalid(Reason::NotUtf8))?;
    let mut inputs: CompileInputs = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let entry = line.split_once('\t').and_then(|(path, package)| {
            Some((RepoPath::new(path).ok()?, PackageName::new(package).ok()?))
        });
        let (path, package) = entry.ok_or_else(|| invalid(Reason::LockLine(index + 1)))?;
        inputs.entry(path).or_default().insert(package);
    }
    Ok(inputs)
}
