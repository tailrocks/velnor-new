// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_model::{
    OptionalSteps, RepoFacts, RepoPath, RustRoot, SkipReason, StepNeed, Target, TargetKind,
};

mod scan;
mod tokens;

use self::scan::Found;
use crate::roots::is_cargo_config;
use crate::select::relative;

#[must_use]
pub fn root_sources(
    facts: &RepoFacts,
    root: &RustRoot,
    tracked: &BTreeSet<RepoPath>,
) -> Option<Vec<RepoPath>> {
    sources_of(facts, root, tracked, true)
}

#[must_use]
pub fn member_sources(
    facts: &RepoFacts,
    root: &RustRoot,
    tracked: &BTreeSet<RepoPath>,
) -> Option<Vec<RepoPath>> {
    sources_of(facts, root, tracked, false)
}

fn sources_of(
    facts: &RepoFacts,
    root: &RustRoot,
    tracked: &BTreeSet<RepoPath>,
    whole_folder: bool,
) -> Option<Vec<RepoPath>> {
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
    if read.iter().any(|input| input.is_none()) {
        return None;
    }
    let manifests: BTreeSet<&RepoPath> = root
        .members
        .iter()
        .map(|member| &member.manifest)
        .chain([&root.manifest])
        .collect();
    let targets = target_sources(root);
    if root
        .members
        .iter()
        .flat_map(|member| &member.targets)
        .filter(|target| target.kinds.iter().copied().any(documented))
        .any(|target| {
            target
                .source
                .as_ref()
                .is_none_or(|source| !tracked.contains(source))
        })
    {
        return None;
    }
    let sources = tracked
        .iter()
        .filter(|path| {
            let config_base = path.parent().and_then(|folder| folder.parent());
            if is_cargo_config(path) {
                return contains(config_base.as_ref(), own.as_ref()) || config_base == own;
            }
            let under = |directory: &Option<RepoPath>| relative(directory.as_ref(), path).is_some();
            let rust = Path::new(path.as_str())
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"));
            manifests.contains(path)
                || targets.contains(path)
                || read
                    .iter()
                    .any(|input| under(input) || input.as_ref() == Some(*path))
                || (rust
                    && ((whole_folder && under(&own) && !nested.iter().any(under))
                        || members.iter().any(under)))
        })
        .cloned()
        .collect();
    Some(sources)
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
    let rust = target_sources(root);
    let other_kinds = undocumented_sources(root);
    let generates = root
        .members
        .iter()
        .flat_map(|member| &member.targets)
        .flat_map(|target| &target.kinds)
        .any(|kind| matches!(kind, TargetKind::ProcMacro | TargetKind::BuildScript));
    for (path, bytes) in sources {
        let text = String::from_utf8_lossy(bytes);
        if is_rust(path, &rust) {
            for include in found.scan_rust(&text, !other_kinds.contains(path)) {
                if let Some(read) = included(sources, path, &include) {
                    found.scan_included(&String::from_utf8_lossy(read));
                }
            }
        } else {
            found.scan_text(&text);
        }
    }
    let unseen = generates || !root.external_dependencies.is_empty();
    let doctests = if !has_doctest_library(root) {
        StepNeed::Skip(SkipReason::NoDoctestLibrary)
    } else if full || unseen || found.fenced {
        StepNeed::Run
    } else {
        StepNeed::Skip(SkipReason::NoFencedDocs)
    };
    let docs = if full || unseen || found.docs {
        StepNeed::Run
    } else {
        StepNeed::Skip(SkipReason::NoDocs)
    };
    let doctests_known = has_doctest_library(root) && found.written;
    OptionalSteps {
        doctests,
        doctests_known,
        docs,
    }
}

fn documented(kind: TargetKind) -> bool {
    matches!(
        kind,
        TargetKind::Lib | TargetKind::ProcMacro | TargetKind::Bin
    )
}

fn target_sources(root: &RustRoot) -> BTreeSet<&RepoPath> {
    root.members
        .iter()
        .flat_map(|member| &member.targets)
        .filter_map(|target| target.source.as_ref())
        .collect()
}

fn undocumented_sources(root: &RustRoot) -> BTreeSet<&RepoPath> {
    let targets = || root.members.iter().flat_map(|member| &member.targets);
    let doctested = |target: &&Target| {
        target.doctest
            && target
                .kinds
                .iter()
                .any(|kind| matches!(kind, TargetKind::Lib | TargetKind::ProcMacro))
    };
    let counted: BTreeSet<_> = targets()
        .filter(doctested)
        .filter_map(|t| t.source.as_ref())
        .collect();
    targets()
        .filter(|target| !doctested(target))
        .filter_map(|target| target.source.as_ref())
        .filter(|source| !counted.contains(source))
        .collect()
}

fn is_rust(path: &RepoPath, targets: &BTreeSet<&RepoPath>) -> bool {
    targets.contains(path)
        || Path::new(path.as_str())
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
}

fn included<'a>(
    sources: &'a BTreeMap<RepoPath, Vec<u8>>,
    from: &RepoPath,
    include: &str,
) -> Option<&'a Vec<u8>> {
    if include.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = from.as_str().split('/').collect();
    parts.pop();
    for part in include.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    let wanted = parts.join("/");
    sources
        .iter()
        .find(|(path, _)| path.as_str() == wanted)
        .map(|(_, bytes)| bytes)
}
