// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use velnor_gen::io::{Checkout, Error as GenError, read_files, read_recorded_checkout};
use velnor_model::{ChangeSet, EventKind, RepoFacts, RepoPath, RustSelection};
use velnor_rust::{
    cargo_files, detect, optional_steps, root_sources, select_event, selects_by_diff,
    unknown_paths, unread_packages,
};

use super::{EVENT_VARIABLE, SCHEDULED, SOURCE_LIMIT, append_outputs, current_dir, git};
use crate::cli::{BaseFile, PlanArgs};
use crate::error::CiError;
use crate::event::{Event, check_default_branch, parse_event};
use crate::plan::{Steps, explain, outputs};

const EVENT_PATH_VARIABLE: &str = "GITHUB_EVENT_PATH";
const LOCK_FILE: &str = "Cargo.lock";

fn ci(error: impl std::fmt::Display) -> CiError {
    CiError::new(error.to_string())
}

pub(super) fn run_plan(args: &PlanArgs) -> Result<String, CiError> {
    let checkout = read_recorded_checkout(&current_dir()?).map_err(ci)?;
    let facts = detect(&checkout.view).map_err(ci)?;
    let event = read_event()?;
    if let Some(event) = &event {
        check_default_branch(&facts.config.default_branch, event)?;
    }
    let name = std::env::var(EVENT_VARIABLE).ok();
    let full = name.as_deref() == Some(SCHEDULED);
    let (selections, changes) = choose(args, &checkout, &facts, name.as_deref(), event.as_ref())?;
    let steps = facts
        .rust_roots
        .iter()
        .map(|root| {
            let steps = match root_sources(&facts, root, &checkout.view.tracked_paths) {
                None => optional_steps(root, &BTreeMap::new(), true),
                Some(paths) => match read_sources(&checkout.root, &paths) {
                    Ok(Some(sources)) => optional_steps(root, &sources, full),
                    Ok(None) | Err(GenError::NotRegular(_)) => {
                        optional_steps(root, &BTreeMap::new(), true)
                    }
                    Err(error) => return Err(ci(error)),
                },
            };
            Ok((root.manifest.clone(), steps))
        })
        .collect::<Result<Steps, CiError>>()?;
    let outputs = outputs(&selections, &steps)?;
    append_outputs(&outputs)?;
    Ok(if args.explain {
        explain(
            &selections,
            &steps,
            &unknown_paths(&facts, &changes),
            &unread_packages(&facts, &changes),
        )
    } else {
        outputs
    })
}

fn event_kind(name: Option<&str>, manual: bool) -> EventKind {
    match name {
        Some("pull_request") => EventKind::PullRequest,
        Some("push") => EventKind::Push,
        Some("merge_group") => EventKind::MergeGroup,
        Some(SCHEDULED) => EventKind::Schedule,
        Some("workflow_dispatch") => EventKind::WorkflowDispatch,
        _ if manual => EventKind::PullRequest,
        _ => EventKind::WorkflowDispatch,
    }
}

fn choose(
    args: &PlanArgs,
    checkout: &Checkout,
    facts: &RepoFacts,
    name: Option<&str>,
    event: Option<&Event>,
) -> Result<(BTreeMap<RepoPath, RustSelection>, ChangeSet), CiError> {
    let kind = event_kind(name, !args.changed.is_empty());
    let diff = if selects_by_diff(kind, &facts.config) {
        changes(args, checkout, facts, kind, event)?
    } else {
        None
    };
    Ok(select_event(facts, kind, || diff))
}

fn changes(
    args: &PlanArgs,
    checkout: &Checkout,
    facts: &RepoFacts,
    kind: EventKind,
    event: Option<&Event>,
) -> Result<Option<ChangeSet>, CiError> {
    let (paths, base_files) = if args.changed.is_empty() {
        match event_changes(checkout, facts, kind, event) {
            Ok(found) => found,
            Err(error) => {
                eprintln!("velnor-ci: {error}; every package is selected");
                return Ok(None);
            }
        }
    } else {
        (
            args.changed.iter().cloned().collect(),
            read_base_files(&args.base_files)?,
        )
    };
    Ok(Some(ChangeSet {
        base_files,
        head_files: read_head_files(checkout, &paths)?,
        paths,
    }))
}

type Found = (BTreeSet<RepoPath>, BTreeMap<RepoPath, String>);

fn event_changes(
    checkout: &Checkout,
    facts: &RepoFacts,
    kind: EventKind,
    event: Option<&Event>,
) -> Result<Found, CiError> {
    let base = match kind {
        EventKind::PullRequest => git::pull_request_base(&checkout.root)?,
        _ => {
            let range = event
                .and_then(|event| event.range.as_ref())
                .ok_or_else(|| CiError::new("the event payload names no commit range"))?;
            git::range_base(&checkout.root, &range.base, &range.head)?
        }
    };
    let paths = git::changed_files(&checkout.root, &base)?;
    let roots: BTreeSet<&RepoPath> = facts.rust_roots.iter().map(|root| &root.manifest).collect();
    let wanted: Vec<RepoPath> = cargo_files(&checkout.view.tracked_paths)
        .into_iter()
        .filter(|path| paths.contains(path) && (is_lock(path) || roots.contains(path)))
        .collect();
    let files = git::base_files(&checkout.root, &base, &wanted);
    Ok((paths, files))
}

fn is_lock(path: &RepoPath) -> bool {
    path.as_str().rsplit('/').next() == Some(LOCK_FILE)
}

fn read_event() -> Result<Option<Event>, CiError> {
    let Some(file) = std::env::var_os(EVENT_PATH_VARIABLE).filter(|file| !file.is_empty()) else {
        return Ok(None);
    };
    let text = fs::read_to_string(&file)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", Path::new(&file).display())))?;
    parse_event(&text).map(Some)
}

fn read_sources(
    root: &Path,
    paths: &[RepoPath],
) -> Result<Option<BTreeMap<RepoPath, Vec<u8>>>, GenError> {
    let oversized = paths.iter().any(|path| {
        fs::symlink_metadata(root.join(path.as_str()))
            .is_ok_and(|metadata| metadata.len() > SOURCE_LIMIT)
    });
    if oversized {
        return Ok(None);
    }
    read_files(root, paths).map(Some)
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
        .filter(|path| changed.contains(path))
        .collect();
    let files = read_files(&checkout.root, &wanted).map_err(ci)?;
    Ok(files
        .into_iter()
        .filter_map(|(path, bytes)| Some((path, String::from_utf8(bytes).ok()?)))
        .collect())
}
