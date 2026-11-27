// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use velnor_gen::io::{Checkout, read_files};
use velnor_model::{ChangeSet, EventKind, RepoFacts, RepoPath};
use velnor_rust::{cargo_files, is_rust_source, is_workflow_file, task_files};

use super::git;
use crate::cli::{BaseFile, PlanArgs};
use crate::error::CiError;
use crate::escape::stderr_line;
use crate::event::Event;

const LOCK_FILE: &str = "Cargo.lock";

pub(super) fn changes(
    args: &PlanArgs,
    checkout: &Checkout,
    facts: &RepoFacts,
    kind: EventKind,
    event: Option<&Event>,
) -> Result<Option<Changed>, CiError> {
    let (paths, base_files, observed) = if args.changed.is_empty() {
        match event_changes(checkout, facts, kind, event) {
            Ok(found) => found,
            Err(error) => {
                eprintln!(
                    "{}",
                    stderr_line(&format!("{error}; every package is selected"))
                );
                return Ok(None);
            }
        }
    } else {
        let paths: BTreeSet<RepoPath> = args.changed.iter().cloned().collect();
        let observed = paths
            .iter()
            .filter(|path| is_rust_source(path))
            .cloned()
            .collect();
        (paths, read_base_files(&args.base_files)?, observed)
    };
    Ok(Some(Changed {
        set: ChangeSet {
            base_files,
            head_files: read_head_files(checkout, &paths)?,
            paths,
        },
        observed,
    }))
}

pub(super) struct Changed {
    pub(super) set: ChangeSet,
    pub(super) observed: BTreeSet<RepoPath>,
}

type Found = (
    BTreeSet<RepoPath>,
    BTreeMap<RepoPath, String>,
    BTreeSet<RepoPath>,
);

fn event_changes(
    checkout: &Checkout,
    facts: &RepoFacts,
    kind: EventKind,
    event: Option<&Event>,
) -> Result<Found, CiError> {
    let base = if kind == EventKind::PullRequest {
        git::pull_request_base(&checkout.root)?
    } else {
        let range = event
            .and_then(|event| event.range.as_ref())
            .ok_or_else(|| CiError::new("the event payload names no commit range"))?;
        git::range_base(&checkout.root, &range.base, &range.head)?
    };
    let paths = git::changed_files(&checkout.root, &base)?;
    let roots: BTreeSet<&RepoPath> = facts.rust_roots.iter().map(|root| &root.manifest).collect();
    let wanted: Vec<RepoPath> = cargo_files(&checkout.view.tracked_paths)
        .into_iter()
        .filter(|path| paths.contains(path) && (is_lock(path) || roots.contains(path)))
        .chain(paths.iter().filter(|path| is_workflow_file(path)).cloned())
        .chain(task_files(facts, &paths).cloned())
        .chain(
            facts
                .weakenings
                .iter()
                .map(|weakening| &weakening.file)
                .filter(|file| paths.contains(file))
                .cloned(),
        )
        .collect::<BTreeSet<RepoPath>>()
        .into_iter()
        .collect();
    let files = git::base_files(&checkout.root, &base, &wanted);
    let observed = git::observed_sources(&checkout.root, &base, &paths)?;
    Ok((paths, files, observed))
}

fn is_lock(path: &RepoPath) -> bool {
    path.as_str().rsplit('/').next() == Some(LOCK_FILE)
}

fn read_base_files(files: &[BaseFile]) -> Result<BTreeMap<RepoPath, String>, CiError> {
    files
        .iter()
        .map(|base| {
            fs::read_to_string(&base.file)
                .map(|text| (base.path.clone(), text))
                .map_err(|e| CiError::new(format!("cannot read {}: {e}", base.file.display())))
        })
        .collect()
}

fn read_head_files(
    checkout: &Checkout,
    changed: &BTreeSet<RepoPath>,
) -> Result<BTreeMap<RepoPath, String>, CiError> {
    let wanted: Vec<RepoPath> = cargo_files(&checkout.view.tracked_paths)
        .into_iter()
        .chain(
            checkout
                .view
                .tracked_paths
                .iter()
                .filter(|path| is_workflow_file(path))
                .cloned(),
        )
        .filter(|path| changed.contains(path))
        .collect();
    let files =
        read_files(&checkout.root, &wanted).map_err(|error| CiError::new(error.to_string()))?;
    Ok(files
        .into_iter()
        .filter_map(|(path, bytes)| Some((path, String::from_utf8(bytes).ok()?)))
        .collect())
}
