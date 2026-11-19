// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use velnor_model::{CompileInputs, InputOwner, PackageName, RepoPath};

use crate::error::{DetectError, Reason};

pub const INPUTS_LOCK: RepoPath = RepoPath::from_static(".velnor/rust-inputs.lock");
/// The first `velnor-ci` that reads a lock line naming its package's manifest.
pub const INPUTS_IDENTITY_HELPER: (u64, u64, u64) = (0, 1, 101);
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

#[must_use]
pub fn unlisted(found: &CompileInputs, listed: &CompileInputs) -> CompileInputs {
    found
        .iter()
        .filter_map(|(path, owners)| {
            let missing: BTreeSet<InputOwner> = owners
                .iter()
                .filter(|owner| {
                    !listed
                        .get(path)
                        .is_some_and(|l| l.iter().any(|entry| entry.same_package(owner)))
                })
                .cloned()
                .collect();
            (!missing.is_empty()).then(|| (path.clone(), missing))
        })
        .collect()
}

/// The inputs with every manifest dropped: the name-only lines a velnor-ci before 0.1.101 reads.
#[must_use]
pub fn without_identity(inputs: &CompileInputs) -> CompileInputs {
    inputs
        .iter()
        .map(|(path, owners)| {
            let named = owners
                .iter()
                .map(|owner| InputOwner::named(owner.name.clone()))
                .collect();
            (path.clone(), named)
        })
        .collect()
}

#[must_use]
pub fn format_inputs_lock(inputs: &CompileInputs) -> String {
    inputs
        .iter()
        .flat_map(|(path, owners)| {
            owners.iter().map(move |owner| {
                let path = path.as_ref().map_or(REPOSITORY, RepoPath::as_str);
                match &owner.manifest {
                    Some(manifest) => format!("{path}\t{}\t{manifest}\n", owner.name),
                    None => format!("{path}\t{}\n", owner.name),
                }
            })
        })
        .collect()
}

/// The lock text a velnor-ci pinned in the repository can read: manifests only if `identified`.
#[must_use]
pub fn inputs_lock_text(inputs: &CompileInputs, identified: bool) -> String {
    if identified {
        format_inputs_lock(inputs)
    } else {
        format_inputs_lock(&without_identity(inputs))
    }
}

/// Whether any line of the lock carries a manifest, which a velnor-ci before 0.1.101 refuses.
#[must_use]
pub fn lock_is_identified(bytes: &[u8]) -> bool {
    bytes
        .split(|byte| *byte == b'\n')
        .any(|line| line.split(|byte| *byte == b'\t').count() >= 3)
}

pub fn parse_inputs_lock(bytes: &[u8]) -> Result<CompileInputs, DetectError> {
    let invalid = |reason| DetectError::invalid(&INPUTS_LOCK, reason);
    let text = std::str::from_utf8(bytes).map_err(|_| invalid(Reason::NotUtf8))?;
    let mut inputs: CompileInputs = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let entry = line.split_once('\t').and_then(|(path, rest)| {
            let path = match path {
                REPOSITORY => None,
                path => Some(RepoPath::new(path).ok()?),
            };
            let (package, manifest) = match rest.split_once('\t') {
                Some((package, manifest)) => (package, Some(RepoPath::new(manifest).ok()?)),
                None => (rest, None),
            };
            let owner = InputOwner {
                name: PackageName::new(package).ok()?,
                manifest,
            };
            Some((path, owner))
        });
        let (path, owner) = entry.ok_or_else(|| invalid(Reason::LockLine(index + 1)))?;
        inputs.entry(path).or_default().insert(owner);
    }
    Ok(inputs)
}
