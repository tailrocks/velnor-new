// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use velnor_model::{CompileInputs, PackageName, RepoPath};

use crate::error::{DetectError, Reason};

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
