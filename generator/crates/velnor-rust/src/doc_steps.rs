// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_model::{
    OptionalSteps, RepoFacts, RepoPath, RustRoot, SkipReason, StepNeed, TargetKind,
};

use crate::select::relative;

#[must_use]
pub fn root_sources(
    facts: &RepoFacts,
    root: &RustRoot,
    tracked: &BTreeSet<RepoPath>,
) -> Vec<RepoPath> {
    let own = root.manifest.parent();
    let nested: Vec<Option<RepoPath>> = facts
        .rust_roots
        .iter()
        .map(|other| other.manifest.parent())
        .filter(|other| *other != own && contains(own.as_ref(), other.as_ref()))
        .collect();
    let members: Vec<Option<RepoPath>> = root
        .members
        .iter()
        .map(|member| member.directory.clone())
        .collect();
    let names: BTreeSet<_> = root.members.iter().map(|member| &member.name).collect();
    let read: Vec<&Option<RepoPath>> = facts
        .compile_inputs
        .iter()
        .filter(|(_, packages)| packages.iter().any(|package| names.contains(package)))
        .map(|(path, _)| path)
        .collect();
    let manifests: BTreeSet<&RepoPath> = root
        .members
        .iter()
        .map(|member| &member.manifest)
        .chain([&root.manifest])
        .collect();
    tracked
        .iter()
        .filter(|path| {
            manifests.contains(path)
                || Path::new(path.as_str())
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
        })
        .filter(|path| {
            manifests.contains(path) || {
                let under =
                    |directory: &Option<RepoPath>| relative(directory.as_ref(), path).is_some();
                (under(&own) && !nested.iter().any(under))
                    || members.iter().any(under)
                    || read
                        .iter()
                        .any(|input| under(input) || input.as_ref() == Some(*path))
            }
        })
        .cloned()
        .collect()
}

fn contains(outer: Option<&RepoPath>, inner: Option<&RepoPath>) -> bool {
    inner.is_some_and(|inner| relative(outer, inner).is_some())
}

pub(crate) fn has_doctest_library(root: &RustRoot) -> bool {
    root.members.iter().flat_map(|m| &m.targets).any(|target| {
        target.doctest
            && target
                .kinds
                .iter()
                .any(|kind| matches!(kind, TargetKind::Lib | TargetKind::ProcMacro))
    })
}

#[must_use]
pub fn optional_steps(
    root: &RustRoot,
    sources: &BTreeMap<RepoPath, Vec<u8>>,
    full: bool,
) -> OptionalSteps {
    let mut found = Found::default();
    for bytes in sources.values() {
        found.scan(&String::from_utf8_lossy(bytes));
    }
    let doctests = if !has_doctest_library(root) {
        StepNeed::Skip(SkipReason::NoDoctestLibrary)
    } else if full || found.fenced {
        StepNeed::Run
    } else {
        StepNeed::Skip(SkipReason::NoFencedDocs)
    };
    let docs = if full || found.docs {
        StepNeed::Run
    } else {
        StepNeed::Skip(SkipReason::NoDocs)
    };
    OptionalSteps { doctests, docs }
}

#[derive(Default)]
struct Found {
    docs: bool,
    fenced: bool,
}

impl Found {
    fn scan(&mut self, text: &str) {
        let mut attribute_depth = 0_usize;
        for line in text.lines().map(str::trim_start) {
            self.docs |= RUSTDOC_SETTINGS
                .iter()
                .any(|setting| line.contains(setting));
            if let Some((rest, block)) = doc_comment(line) {
                self.docs = true;
                self.fenced |= block || code_block(rest);
            } else if attribute_depth > 0 || line.starts_with("#[") || line.starts_with("#![") {
                let opened = line.matches('[').count();
                let closed = line.matches(']').count();
                attribute_depth = (attribute_depth + opened).saturating_sub(closed);
                if doc_attribute(line) {
                    self.docs = true;
                    self.fenced = true;
                }
            }
        }
    }
}

const RUSTDOC_SETTINGS: [&str; 2] = ["rustdoc", "missing_docs"];

fn code_block(rest: &str) -> bool {
    let text = rest.strip_prefix(' ').unwrap_or(rest);
    let trimmed = text.trim_start();
    trimmed.starts_with("```")
        || trimmed.starts_with("~~~")
        || text.starts_with("    ")
        || text.starts_with('\t')
}

fn doc_comment(line: &str) -> Option<(&str, bool)> {
    if let Some(rest) = line.strip_prefix("///") {
        (!rest.starts_with('/')).then_some((rest, false))
    } else if let Some(rest) = line.strip_prefix("//!") {
        Some((rest, false))
    } else if let Some(rest) = line.strip_prefix("/**") {
        (!rest.starts_with(['*', '/'])).then_some((rest, true))
    } else {
        line.strip_prefix("/*!").map(|rest| (rest, true))
    }
}

fn doc_attribute(line: &str) -> bool {
    line.match_indices("doc").any(|(at, _)| {
        let before = line[..at].chars().next_back();
        let after = line[at + 3..].trim_start();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '_') && after.starts_with('=')
    })
}
